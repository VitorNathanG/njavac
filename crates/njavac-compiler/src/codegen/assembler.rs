use crate::classfile::{StackFrame, VerificationType};

use super::instruction::{
    CodePosition, Instruction, InstructionAnchor, Label, GOTO, IINC, WIDE,
};

#[derive(Clone, Copy)]
struct InstructionEntry {
    instruction: Instruction,
    live: bool,
}

#[derive(Clone, Copy)]
struct LabelBinding {
    position: Option<CodePosition>,
}

struct LineEvent {
    instruction: InstructionAnchor,
    line: u16,
}

struct FrameReq {
    position: CodePosition,
    locals: Vec<VerificationType>,
    stack: Vec<VerificationType>,
}

pub(super) struct AssembledCode {
    pub(super) code: Vec<u8>,
    pub(super) line_numbers: Vec<(u16, u16)>,
    pub(super) stack_frames: Vec<StackFrame>,
    pub(super) max_stack: u16,
}

/// Symbolic per-method bytecode state. This is the single path for instruction
/// recording, pending-line consumption, and stack accounting; `finish` owns the
/// one final layout and encoding pass.
pub(super) struct Emitter {
    instructions: Vec<InstructionEntry>,
    labels: Vec<LabelBinding>,
    line_events: Vec<LineEvent>,
    frames: Vec<FrameReq>,
    pending_line: Option<u16>,
    at_control_entry: bool,
    max_stack: u16,
    cur: u16,
}

impl Emitter {
    pub(super) fn new() -> Self {
        Emitter {
            instructions: Vec::with_capacity(32),
            labels: Vec::new(),
            line_events: Vec::with_capacity(16),
            frames: Vec::new(),
            pending_line: None,
            at_control_entry: false,
            max_stack: 0,
            cur: 0,
        }
    }

    pub(super) fn emit(&mut self, instruction: Instruction) -> InstructionAnchor {
        let anchor = InstructionAnchor(self.instructions.len());
        if let Some(line) = self.pending_line.take() {
            if self.line_events.last().map(|event| event.line) != Some(line) {
                self.line_events.push(LineEvent {
                    instruction: anchor,
                    line,
                });
            }
        }
        self.at_control_entry = false;
        self.instructions.push(InstructionEntry {
            instruction,
            live: true,
        });

        let effect = instruction.stack_effect();
        self.cur = self
            .cur
            .checked_sub(effect.pop)
            .unwrap_or_else(|| panic!("operand-stack underflow at instruction {}", anchor.0));
        self.cur = self
            .cur
            .checked_add(effect.push)
            .unwrap_or_else(|| panic!("operand-stack overflow at instruction {}", anchor.0));
        self.max_stack = self.max_stack.max(self.cur);
        anchor
    }

    pub(super) fn position(&self) -> CodePosition {
        CodePosition(self.instructions.len())
    }

    pub(super) fn new_label(&mut self) -> Label {
        let label = Label(self.labels.len());
        self.labels.push(LabelBinding { position: None });
        label
    }

    pub(super) fn place_label(&mut self, label: Label) {
        let position = self.position();
        let binding = &mut self.labels[label.0];
        debug_assert!(binding.position.is_none(), "branch label placed twice");
        binding.position = Some(position);
    }

    pub(super) fn emit_branch(&mut self, opcode: u8, target: Label) -> InstructionAnchor {
        self.emit(Instruction::Branch { opcode, target })
    }

    pub(super) fn instruction_count(&self) -> usize {
        self.instructions.len()
    }

    pub(super) fn stack_depth(&self) -> u16 {
        self.cur
    }

    pub(super) fn reset_stack(&mut self) {
        self.cur = 0;
    }

    pub(super) fn pending_line(&self) -> Option<u16> {
        self.pending_line
    }

    pub(super) fn set_pending_line(&mut self, line: Option<u16>) {
        self.pending_line = line;
    }

    pub(super) fn at_control_entry(&self) -> bool {
        self.at_control_entry
    }

    pub(super) fn retarget_branches(&mut self, from: Label, to: Label) {
        for entry in &mut self.instructions {
            if let Instruction::Branch { target, .. } = &mut entry.instruction {
                if *target == from {
                    *target = to;
                }
            }
        }
    }

    pub(super) fn request_frame(
        &mut self,
        locals: Vec<VerificationType>,
        stack: Vec<VerificationType>,
    ) {
        self.at_control_entry = true;
        self.frames.push(FrameReq {
            position: self.position(),
            locals,
            stack,
        });
    }

    fn label_position(&self, label: Label) -> CodePosition {
        self.labels[label.0]
            .position
            .unwrap_or_else(|| panic!("unplaced branch label {}", label.0))
    }

    fn next_live_position(&self, position: CodePosition) -> CodePosition {
        let mut index = position.0;
        while index < self.instructions.len() && !self.instructions[index].live {
            index += 1;
        }
        CodePosition(index)
    }

    /// Follow unconditional gotos from a symbolic boundary to the final live
    /// non-goto boundary. The bound also guards malformed goto cycles.
    fn thread_from_position(&self, start: CodePosition) -> CodePosition {
        let mut position = self.next_live_position(start);
        for _ in 0..=self.instructions.len() {
            let Some(entry) = self.instructions.get(position.0).filter(|entry| entry.live) else {
                break;
            };
            let Instruction::Branch {
                opcode: GOTO,
                target,
            } = entry.instruction
            else {
                break;
            };
            let next = self.next_live_position(self.label_position(target));
            if next == position {
                break;
            }
            position = next;
        }
        position
    }

    fn thread_target(&self, label: Label) -> CodePosition {
        self.thread_from_position(self.label_position(label))
    }

    /// Delete only unreachable and goto-to-next gotos, preserving javac's
    /// observed fixpoint behavior. Tombstones keep every symbolic anchor stable.
    fn compact_gotos(&mut self) {
        if !self
            .instructions
            .iter()
            .any(|entry| entry.live && entry.instruction.is_goto())
        {
            return;
        }

        #[cfg(debug_assertions)]
        self.assert_compaction_preconditions();

        loop {
            let n = self.instructions.len();
            let mut reachable = vec![false; n];
            let mut work = vec![self.next_live_position(CodePosition(0))];
            while let Some(position) = work.pop() {
                let index = position.0;
                if index >= n || reachable[index] || !self.instructions[index].live {
                    continue;
                }
                reachable[index] = true;
                let instruction = self.instructions[index].instruction;
                match instruction {
                    Instruction::Branch { target, .. } if instruction.is_goto() => {
                        work.push(self.thread_target(target));
                    }
                    Instruction::Branch { target, .. } if instruction.is_cond_branch() => {
                        work.push(self.thread_target(target));
                        work.push(self.next_live_position(CodePosition(index + 1)));
                    }
                    _ if instruction.is_return() => {}
                    _ => work.push(self.next_live_position(CodePosition(index + 1))),
                }
            }

            let mut dead = Vec::new();
            for (index, entry) in self.instructions.iter().enumerate() {
                if !entry.live || !entry.instruction.is_goto() {
                    continue;
                }
                let Instruction::Branch { target, .. } = entry.instruction else {
                    unreachable!()
                };
                if !reachable[index]
                    || self.thread_target(target)
                        == self.next_live_position(CodePosition(index + 1))
                {
                    dead.push(index);
                }
            }
            if dead.is_empty() {
                break;
            }

            let threaded_labels: Vec<Option<CodePosition>> = self
                .labels
                .iter()
                .map(|binding| {
                    binding
                        .position
                        .map(|position| self.thread_from_position(position))
                })
                .collect();

            for &index in &dead {
                debug_assert!(
                    self.frames.iter().all(|frame| frame.position.0 != index),
                    "frame at a deleted goto"
                );
                self.instructions[index].live = false;
            }

            let normalized_labels: Vec<Option<CodePosition>> = threaded_labels
                .into_iter()
                .map(|position| position.map(|position| self.next_live_position(position)))
                .collect();
            for (binding, position) in self.labels.iter_mut().zip(normalized_labels) {
                binding.position = position;
            }
        }
    }

    #[cfg(debug_assertions)]
    fn assert_compaction_preconditions(&self) {
        for frame in &self.frames {
            debug_assert!(
                self.instructions
                    .get(frame.position.0)
                    .is_none_or(|entry| !entry.instruction.is_goto()),
                "frame requested at a goto"
            );
        }
    }

    fn layout(&self) -> Vec<u32> {
        let mut pcs = Vec::with_capacity(self.instructions.len() + 1);
        let mut pc = 0u32;
        for entry in &self.instructions {
            pcs.push(pc);
            if entry.live {
                pc = pc
                    .checked_add(entry.instruction.encoded_len() as u32)
                    .expect("method code length overflow");
            }
        }
        pcs.push(pc);
        assert!(
            pc <= u16::MAX as u32,
            "method code exceeds JVM Code attribute limit"
        );
        pcs
    }

    fn resolve_lines(&self, pcs: &[u32]) -> Vec<(u16, u16)> {
        let mut out = Vec::with_capacity(self.line_events.len());
        for event in &self.line_events {
            if !self.instructions[event.instruction.0].live {
                continue;
            }
            if out.last().map(|&(_, line)| line) != Some(event.line) {
                out.push((pcs[event.instruction.0] as u16, event.line));
            }
        }
        out
    }

    fn live_target_pcs(&self, pcs: &[u32]) -> std::collections::HashSet<u32> {
        self.instructions
            .iter()
            .filter(|entry| entry.live)
            .filter_map(|entry| match entry.instruction {
                Instruction::Branch { target, .. } => Some(pcs[self.thread_target(target).0]),
                _ => None,
            })
            .collect()
    }

    fn resolve_frames(
        &mut self,
        pcs: &[u32],
        live_targets: &std::collections::HashSet<u32>,
    ) -> Vec<StackFrame> {
        self.frames.sort_by_key(|frame| pcs[frame.position.0]);
        let mut out: Vec<StackFrame> = Vec::new();
        for frame in &self.frames {
            let offset = pcs[frame.position.0];
            if !live_targets.contains(&offset) {
                continue;
            }
            let offset = offset as u16;
            if let Some(previous) = out.last().filter(|previous| previous.offset == offset) {
                debug_assert_eq!(
                    (&previous.locals, &previous.stack),
                    (&frame.locals, &frame.stack),
                    "conflicting frame states requested at pc {offset}"
                );
                continue;
            }
            out.push(StackFrame {
                offset,
                locals: frame.locals.clone(),
                stack: frame.stack.clone(),
            });
        }
        out
    }

    fn encode(&self, pcs: &[u32]) -> Vec<u8> {
        let mut code = Vec::with_capacity(*pcs.last().unwrap() as usize);
        for (index, entry) in self.instructions.iter().enumerate() {
            if !entry.live {
                continue;
            }
            debug_assert_eq!(code.len(), pcs[index] as usize);
            let before = code.len();
            match entry.instruction {
                Instruction::Simple(opcode) => code.push(opcode),
                Instruction::U8 { opcode, operand } => {
                    code.push(opcode);
                    code.push(operand);
                }
                Instruction::U16 { opcode, operand }
                | Instruction::Field {
                    opcode,
                    index: operand,
                    ..
                }
                | Instruction::Invoke {
                    opcode,
                    index: operand,
                    ..
                } => {
                    code.push(opcode);
                    push_u16(&mut code, operand);
                }
                Instruction::Iinc { slot, delta } => {
                    code.push(IINC);
                    code.push(slot);
                    code.push(delta as u8);
                }
                Instruction::WideLocal { opcode, slot } => {
                    code.push(WIDE);
                    code.push(opcode);
                    push_u16(&mut code, slot);
                }
                Instruction::WideIinc { slot, delta } => {
                    code.push(WIDE);
                    code.push(IINC);
                    push_u16(&mut code, slot);
                    push_u16(&mut code, delta as u16);
                }
                Instruction::Branch { opcode, target } => {
                    let target_pc = pcs[self.thread_target(target).0] as i64;
                    let branch_pc = pcs[index] as i64;
                    let offset = i16::try_from(target_pc - branch_pc)
                        .expect("branch offset exceeds selected narrow form");
                    code.push(opcode);
                    code.extend_from_slice(&offset.to_be_bytes());
                }
            }
            debug_assert_eq!(code.len() - before, entry.instruction.encoded_len());
        }
        debug_assert_eq!(code.len(), *pcs.last().unwrap() as usize);
        code
    }

    pub(super) fn finish(mut self) -> AssembledCode {
        self.compact_gotos();
        let pcs = self.layout();
        let live_targets = self.live_target_pcs(&pcs);
        let line_numbers = self.resolve_lines(&pcs);
        let stack_frames = self.resolve_frames(&pcs, &live_targets);
        let code = self.encode(&pcs);
        AssembledCode {
            code,
            line_numbers,
            stack_frames,
            max_stack: self.max_stack,
        }
    }
}

fn push_u16(code: &mut Vec<u8>, v: u16) {
    code.extend_from_slice(&v.to_be_bytes());
}

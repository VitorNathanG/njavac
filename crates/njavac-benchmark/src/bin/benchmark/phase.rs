use std::fmt;

use njavac_compiler::CompilePhase;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum PhaseName {
    Lex,
    Parse,
    SemanticAnalysis,
    CodegenPlanning,
    ClassfileSerializationAndPlanDrop,
    AnalysisAndSyntaxDrop,
    ResultBytesDrop,
}

impl PhaseName {
    pub(super) const ALL: [Self; 7] = [
        Self::Lex,
        Self::Parse,
        Self::SemanticAnalysis,
        Self::CodegenPlanning,
        Self::ClassfileSerializationAndPlanDrop,
        Self::AnalysisAndSyntaxDrop,
        Self::ResultBytesDrop,
    ];

    pub(super) const COMPILER: [Self; 6] = [
        Self::Lex,
        Self::Parse,
        Self::SemanticAnalysis,
        Self::CodegenPlanning,
        Self::ClassfileSerializationAndPlanDrop,
        Self::AnalysisAndSyntaxDrop,
    ];

    pub(super) fn as_str(self) -> &'static str {
        match self {
            Self::Lex => "lex",
            Self::Parse => "parse",
            Self::SemanticAnalysis => "semantic_analysis",
            Self::CodegenPlanning => "codegen_planning",
            Self::ClassfileSerializationAndPlanDrop => "classfile_serialization_and_plan_drop",
            Self::AnalysisAndSyntaxDrop => "analysis_and_syntax_drop",
            Self::ResultBytesDrop => "result_bytes_drop",
        }
    }

    pub(super) fn from_protocol(value: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|phase| phase.as_str() == value)
    }
}

impl From<CompilePhase> for PhaseName {
    fn from(value: CompilePhase) -> Self {
        match value {
            CompilePhase::Lex => Self::Lex,
            CompilePhase::Parse => Self::Parse,
            CompilePhase::Sema => Self::SemanticAnalysis,
            CompilePhase::CodegenPlan => Self::CodegenPlanning,
            CompilePhase::ClassfileEmit => Self::ClassfileSerializationAndPlanDrop,
            CompilePhase::Cleanup => Self::AnalysisAndSyntaxDrop,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct PhaseValues<T> {
    pub lex: T,
    pub parse: T,
    pub semantic_analysis: T,
    pub codegen_planning: T,
    pub classfile_serialization_and_plan_drop: T,
    pub analysis_and_syntax_drop: T,
    pub result_bytes_drop: T,
}

impl<T> PhaseValues<T> {
    pub(super) fn get(&self, phase: PhaseName) -> &T {
        match phase {
            PhaseName::Lex => &self.lex,
            PhaseName::Parse => &self.parse,
            PhaseName::SemanticAnalysis => &self.semantic_analysis,
            PhaseName::CodegenPlanning => &self.codegen_planning,
            PhaseName::ClassfileSerializationAndPlanDrop => {
                &self.classfile_serialization_and_plan_drop
            }
            PhaseName::AnalysisAndSyntaxDrop => &self.analysis_and_syntax_drop,
            PhaseName::ResultBytesDrop => &self.result_bytes_drop,
        }
    }

    pub(super) fn get_mut(&mut self, phase: PhaseName) -> &mut T {
        match phase {
            PhaseName::Lex => &mut self.lex,
            PhaseName::Parse => &mut self.parse,
            PhaseName::SemanticAnalysis => &mut self.semantic_analysis,
            PhaseName::CodegenPlanning => &mut self.codegen_planning,
            PhaseName::ClassfileSerializationAndPlanDrop => {
                &mut self.classfile_serialization_and_plan_drop
            }
            PhaseName::AnalysisAndSyntaxDrop => &mut self.analysis_and_syntax_drop,
            PhaseName::ResultBytesDrop => &mut self.result_bytes_drop,
        }
    }

    pub(super) fn try_map<U, E>(
        self,
        mut map: impl FnMut(T) -> Result<U, E>,
    ) -> Result<PhaseValues<U>, E> {
        Ok(PhaseValues {
            lex: map(self.lex)?,
            parse: map(self.parse)?,
            semantic_analysis: map(self.semantic_analysis)?,
            codegen_planning: map(self.codegen_planning)?,
            classfile_serialization_and_plan_drop: map(self.classfile_serialization_and_plan_drop)?,
            analysis_and_syntax_drop: map(self.analysis_and_syntax_drop)?,
            result_bytes_drop: map(self.result_bytes_drop)?,
        })
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum SequenceError {
    Nested {
        active: PhaseName,
        started: PhaseName,
    },
    UnexpectedStart {
        expected: PhaseName,
        actual: PhaseName,
    },
    FinishWithoutStart(PhaseName),
    UnexpectedFinish {
        active: PhaseName,
        actual: PhaseName,
    },
    Incomplete {
        expected: PhaseName,
    },
}

impl fmt::Display for SequenceError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Nested { active, started } => write!(
                formatter,
                "phase {} started while {} was active",
                started.as_str(),
                active.as_str(),
            ),
            Self::UnexpectedStart { expected, actual } => write!(
                formatter,
                "expected phase {}, got {}",
                expected.as_str(),
                actual.as_str(),
            ),
            Self::FinishWithoutStart(phase) => {
                write!(
                    formatter,
                    "phase {} finished without starting",
                    phase.as_str()
                )
            }
            Self::UnexpectedFinish { active, actual } => write!(
                formatter,
                "phase {} finished while {} was active",
                actual.as_str(),
                active.as_str(),
            ),
            Self::Incomplete { expected } => {
                write!(
                    formatter,
                    "successful compilation omitted phase {}",
                    expected.as_str()
                )
            }
        }
    }
}

#[derive(Default)]
pub(super) struct SequenceValidator {
    next: usize,
    active: Option<PhaseName>,
    error: Option<SequenceError>,
}

impl SequenceValidator {
    pub(super) fn started(&mut self, phase: CompilePhase) {
        if self.error.is_some() {
            return;
        }
        let phase = PhaseName::from(phase);
        if let Some(active) = self.active {
            self.error = Some(SequenceError::Nested {
                active,
                started: phase,
            });
            return;
        }
        let expected = PhaseName::COMPILER.get(self.next).copied();
        if expected != Some(phase) {
            self.error = Some(SequenceError::UnexpectedStart {
                expected: expected.unwrap_or(PhaseName::AnalysisAndSyntaxDrop),
                actual: phase,
            });
            return;
        }
        self.active = Some(phase);
    }

    pub(super) fn finished(&mut self, phase: CompilePhase) {
        if self.error.is_some() {
            return;
        }
        let phase = PhaseName::from(phase);
        match self.active {
            None => self.error = Some(SequenceError::FinishWithoutStart(phase)),
            Some(active) if active != phase => {
                self.error = Some(SequenceError::UnexpectedFinish {
                    active,
                    actual: phase,
                });
            }
            Some(_) => {
                self.active = None;
                self.next += 1;
            }
        }
    }

    pub(super) fn complete_success(&mut self) -> Result<(), SequenceError> {
        if let Some(error) = self.error.take() {
            self.reset();
            return Err(error);
        }
        if let Some(active) = self.active {
            self.reset();
            return Err(SequenceError::UnexpectedFinish {
                active,
                actual: active,
            });
        }
        if self.next != PhaseName::COMPILER.len() {
            let expected = PhaseName::COMPILER[self.next];
            self.reset();
            return Err(SequenceError::Incomplete { expected });
        }
        self.reset();
        Ok(())
    }

    #[allow(dead_code)]
    pub(super) fn complete_error(&mut self) -> Result<(), SequenceError> {
        if let Some(error) = self.error.take() {
            self.reset();
            return Err(error);
        }
        if let Some(active) = self.active {
            self.reset();
            return Err(SequenceError::UnexpectedFinish {
                active,
                actual: active,
            });
        }
        self.reset();
        Ok(())
    }

    fn reset(&mut self) {
        self.next = 0;
        self.active = None;
        self.error = None;
    }
}

#[cfg(test)]
mod tests {
    use super::{PhaseName, SequenceValidator};
    use njavac_compiler::CompilePhase;

    #[test]
    fn accepts_the_exact_compiler_sequence() {
        let mut sequence = SequenceValidator::default();
        for phase in [
            CompilePhase::Lex,
            CompilePhase::Parse,
            CompilePhase::Sema,
            CompilePhase::CodegenPlan,
            CompilePhase::ClassfileEmit,
            CompilePhase::Cleanup,
        ] {
            sequence.started(phase);
            sequence.finished(phase);
        }
        assert_eq!(sequence.complete_success(), Ok(()));
    }

    #[test]
    fn rejects_missing_duplicate_nested_and_out_of_order_events() {
        let mut missing = SequenceValidator::default();
        missing.started(CompilePhase::Lex);
        missing.finished(CompilePhase::Lex);
        assert!(missing.complete_success().is_err());

        let mut duplicate = SequenceValidator::default();
        duplicate.started(CompilePhase::Lex);
        duplicate.finished(CompilePhase::Lex);
        duplicate.started(CompilePhase::Lex);
        assert!(duplicate.complete_error().is_err());

        let mut nested = SequenceValidator::default();
        nested.started(CompilePhase::Lex);
        nested.started(CompilePhase::Parse);
        assert!(nested.complete_error().is_err());

        let mut out_of_order = SequenceValidator::default();
        out_of_order.started(CompilePhase::Parse);
        assert!(out_of_order.complete_error().is_err());

        assert_eq!(
            PhaseName::from_protocol("result_bytes_drop"),
            Some(PhaseName::ResultBytesDrop)
        );
        assert_eq!(PhaseName::from_protocol("unknown"), None);
    }
}

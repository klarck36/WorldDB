//! Reusable paired-world assertions for public query observations.
//!
//! This module is compiled only for tests. Callers supply two worlds that share the same
//! public state and differ only in caller-hidden data, then project each execution to its
//! public response before comparing it.

/// Observable continuation behavior of a paged public response.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum CursorObservation {
    /// The response does not carry a continuation cursor.
    Absent,
    /// Another page is available.
    Continue,
    /// The cursor reaches the natural end.
    End,
    /// The cursor is uniformly invalidated.
    Invalidated,
}

/// Which public component differed between the paired executions.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum NonInterferenceDifference {
    /// One response succeeded while the other failed.
    OutcomeKind,
    /// Public result values or counts differed.
    Value,
    /// Public response shape differed.
    Shape,
    /// Public error codes differed.
    ErrorCode,
    /// Public error shape differed.
    ErrorShape,
    /// Cursor continuation behavior differed.
    Cursor,
}

/// A redacted projection of a public failure.
///
/// `code` is the stable public code. Shape contains only public field names, never values or
/// internal causes.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct PublicFailure {
    code: String,
    shape: Vec<String>,
}

impl PublicFailure {
    /// Creates a public error projection from a stable code and public field-name shape.
    pub(crate) fn new(code: impl Into<String>, mut shape: Vec<String>) -> Self {
        shape.sort_unstable();
        Self {
            code: code.into(),
            shape,
        }
    }
}

/// Only the caller-visible projection of a query or cursor outcome.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct PublicObservation<T> {
    value: Option<T>,
    failure: Option<PublicFailure>,
    shape: Vec<String>,
    cursor: CursorObservation,
}

impl<T> PublicObservation<T> {
    /// Records a successful value/count and the public response field names.
    pub(crate) fn success(value: T, mut shape: Vec<String>, cursor: CursorObservation) -> Self {
        shape.sort_unstable();
        Self {
            value: Some(value),
            failure: None,
            shape,
            cursor,
        }
    }

    /// Records only safe public error information and the public envelope shape.
    pub(crate) fn failure(
        failure: PublicFailure,
        mut shape: Vec<String>,
        cursor: CursorObservation,
    ) -> Self {
        shape.sort_unstable();
        Self {
            value: None,
            failure: Some(failure),
            shape,
            cursor,
        }
    }
}

/// Two worlds with common caller-visible state and alternate hidden state.
///
/// The type cannot prove that only hidden state differs; fixture construction is responsible
/// for that premise. The observer receives each hidden variant separately, and comparison
/// reports only a public output dimension without formatting either observation.
pub(crate) struct PairedWorld<Visible, Hidden> {
    visible: Visible,
    baseline_hidden: Hidden,
    alternate_hidden: Hidden,
}

impl<Visible, Hidden> PairedWorld<Visible, Hidden> {
    /// Creates a pair that shares `visible` state and varies only its hidden projection.
    pub(crate) const fn new(
        visible: Visible,
        baseline_hidden: Hidden,
        alternate_hidden: Hidden,
    ) -> Self {
        Self {
            visible,
            baseline_hidden,
            alternate_hidden,
        }
    }

    /// Runs both worlds and requires every public output axis to match.
    pub(crate) fn compare<T: Eq>(
        &self,
        observe: impl Fn(&Visible, &Hidden) -> PublicObservation<T>,
    ) -> Result<(), NonInterferenceDifference> {
        let baseline = observe(&self.visible, &self.baseline_hidden);
        let alternate = observe(&self.visible, &self.alternate_hidden);
        if baseline.value.is_some() != alternate.value.is_some() {
            return Err(NonInterferenceDifference::OutcomeKind);
        }
        if baseline.value != alternate.value {
            return Err(NonInterferenceDifference::Value);
        }
        if baseline.shape != alternate.shape {
            return Err(NonInterferenceDifference::Shape);
        }
        if baseline.failure.as_ref().map(|failure| &failure.code)
            != alternate.failure.as_ref().map(|failure| &failure.code)
        {
            return Err(NonInterferenceDifference::ErrorCode);
        }
        if baseline.failure.as_ref().map(|failure| &failure.shape)
            != alternate.failure.as_ref().map(|failure| &failure.shape)
        {
            return Err(NonInterferenceDifference::ErrorShape);
        }
        if baseline.cursor != alternate.cursor {
            return Err(NonInterferenceDifference::Cursor);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::{
        CursorObservation, NonInterferenceDifference, PairedWorld, PublicFailure, PublicObservation,
    };

    #[test]
    fn paired_world_compares_values_shapes_public_errors_and_cursor_behavior() {
        let world = PairedWorld::new("visible", false, true);
        assert_eq!(
            world.compare(|visible, _hidden| {
                PublicObservation::success(
                    visible.len(),
                    vec!["count".to_owned()],
                    CursorObservation::End,
                )
            }),
            Ok(())
        );
        assert_eq!(
            world.compare(|_visible, hidden| {
                PublicObservation::success(
                    usize::from(*hidden),
                    vec!["count".to_owned()],
                    CursorObservation::End,
                )
            }),
            Err(NonInterferenceDifference::Value)
        );
        assert_eq!(
            world.compare(|_visible, hidden| {
                let failure = PublicFailure::new(
                    if *hidden { "E_HIDDEN" } else { "E_UNKNOWN" },
                    vec!["code".to_owned()],
                );
                PublicObservation::<()>::failure(
                    failure,
                    vec!["error".to_owned()],
                    CursorObservation::Invalidated,
                )
            }),
            Err(NonInterferenceDifference::ErrorCode)
        );
        assert_eq!(
            world.compare(|_visible, hidden| {
                PublicObservation::success(
                    (),
                    if *hidden {
                        vec!["id".to_owned(), "value".to_owned()]
                    } else {
                        vec!["value".to_owned()]
                    },
                    if *hidden {
                        CursorObservation::Continue
                    } else {
                        CursorObservation::End
                    },
                )
            }),
            Err(NonInterferenceDifference::Shape)
        );
        assert_eq!(
            world.compare(|_visible, hidden| {
                PublicObservation::success(
                    (),
                    vec!["result".to_owned()],
                    if *hidden {
                        CursorObservation::Continue
                    } else {
                        CursorObservation::End
                    },
                )
            }),
            Err(NonInterferenceDifference::Cursor)
        );
        assert_eq!(
            world.compare(|_visible, hidden| {
                if *hidden {
                    PublicObservation::<()>::failure(
                        PublicFailure::new("E_UNKNOWN", vec!["code".to_owned()]),
                        vec!["error".to_owned()],
                        CursorObservation::Invalidated,
                    )
                } else {
                    PublicObservation::success(
                        (),
                        vec!["result".to_owned()],
                        CursorObservation::Absent,
                    )
                }
            }),
            Err(NonInterferenceDifference::OutcomeKind)
        );
        assert_eq!(
            world.compare(|_visible, hidden| {
                PublicObservation::<()>::failure(
                    PublicFailure::new(
                        "E_UNKNOWN",
                        if *hidden {
                            vec!["code".to_owned(), "retry".to_owned()]
                        } else {
                            vec!["code".to_owned()]
                        },
                    ),
                    vec!["error".to_owned()],
                    CursorObservation::Invalidated,
                )
            }),
            Err(NonInterferenceDifference::ErrorShape)
        );
    }
}

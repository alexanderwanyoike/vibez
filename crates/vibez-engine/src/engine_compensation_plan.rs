//! Applied-plan validity and structural replacement expectation retain independent lifetimes.

#[derive(Default, PartialEq, Eq)]
enum AppliedPlanValidity {
    #[default]
    Valid,
    Invalid,
}

#[derive(Default, PartialEq, Eq)]
enum StructuralPlanExpectation {
    #[default]
    Current,
    Awaiting,
}

#[derive(Default)]
pub(super) struct CompensationPlanState {
    // Rejecting a replacement leaves invalidity and its structural request
    // alive together; neither transition may erase the other lifetime.
    validity: AppliedPlanValidity,
    expectation: StructuralPlanExpectation,
}

impl CompensationPlanState {
    pub(super) fn applied_plan_is_valid(&self) -> bool {
        self.validity == AppliedPlanValidity::Valid
    }

    pub(super) fn awaiting_structural_plan(&self) -> bool {
        self.expectation == StructuralPlanExpectation::Awaiting
    }

    pub(super) fn blocks_processing(&self) -> bool {
        self.awaiting_structural_plan() || !self.applied_plan_is_valid()
    }

    pub(super) fn expect_structural_plan(&mut self) {
        self.expectation = StructuralPlanExpectation::Awaiting;
    }

    pub(super) fn invalidate_applied_plan(&mut self) {
        self.validity = AppliedPlanValidity::Invalid;
    }

    pub(super) fn accept_full_plan(&mut self) {
        self.expectation = StructuralPlanExpectation::Current;
        self.validity = AppliedPlanValidity::Valid;
    }
}

//! The input and delta movement at the entry to a living entity's travel branch.

/// Live motion at a `living_travel` invocation, before its selected branch runs.
///
/// Read the current values, or replace either vector independently. The ordinary
/// travel code still owns input normalization, collision and movement.
// no wire: this is an exclusive loan of in-process travel state.
// exhaustive: the engine constructs this scoped loan.
pub struct TravelMotion<'a> {
    input: [&'a mut f64; 3],
    velocity: [&'a mut f64; 3],
}

impl<'a> TravelMotion<'a> {
    /// Lend the actual travel argument and the body's delta movement.
    // hidden: the engine lends the resolved vectors at the travel entry point.
    #[doc(hidden)]
    pub fn new(input: [&'a mut f64; 3], velocity: [&'a mut f64; 3]) -> Self {
        Self { input, velocity }
    }

    /// The current input to the selected branch, before vanilla normalizes it.
    pub fn input(&self) -> [f64; 3] {
        self.input.each_ref().map(|axis| **axis)
    }

    /// The body's current delta movement in blocks per tick.
    pub fn velocity(&self) -> [f64; 3] {
        self.velocity.each_ref().map(|axis| **axis)
    }

    /// Replace the travel input. Returns `false` and reports the refusal if any component
    /// is non-finite; the current input and any separate velocity edit or cancellation stand.
    pub fn set_input(&mut self, input: [f64; 3]) -> bool {
        write_vector(&mut self.input, input, "input")
    }

    /// Set the body's delta movement, as in vanilla's `Entity.setDeltaMovement`.
    /// Returns `false` and reports the refusal if any component is non-finite. On success
    /// this write stands even if the listener also cancels the travel branch.
    pub fn set_velocity(&mut self, velocity: [f64; 3]) -> bool {
        write_vector(&mut self.velocity, velocity, "velocity")
    }
}

fn write_vector(target: &mut [&mut f64; 3], value: [f64; 3], field: &str) -> bool {
    if !value.into_iter().all(f64::is_finite) {
        let owner = crate::content::crash::current_native_mod().unwrap_or("unknown");
        log::warn!("mod {owner}: living_travel {field} write refused: components must be finite");
        return false;
    }
    for (axis, value) in target.iter_mut().zip(value) {
        **axis = value;
    }
    true
}

pub struct LandingDeferred<'a>(pub std::marker::PhantomData<&'a mut ()>);

impl LandingDeferred<'_> {
    pub fn checkpoint(&self) -> usize {
        0
    }

    pub fn rollback(&mut self, _len: usize) {}
}

mod api {
    #[derive(Clone, Copy)]
    pub enum BlockScreenEffect {}

    impl super::ExtractBlockScreenEffectEvent<'_> {
        pub fn cancelled(&self) -> bool {
            self.renderer.cancelled()
        }

        pub fn set_cancelled(&mut self, cancelled: bool) {
            self.renderer.set_cancelled(cancelled);
        }
    }
}

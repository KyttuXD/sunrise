pub mod __private {
    #[derive(Clone)]
    pub struct BlockScreenEffectExtraction(bool);

    impl BlockScreenEffectExtraction {
        pub fn cancelled(&self) -> bool {
            self.0
        }

        pub fn set_cancelled(&mut self, cancelled: bool) {
            self.0 = cancelled;
        }
    }

    pub use super::ConfigurationTaskRegistration;
}

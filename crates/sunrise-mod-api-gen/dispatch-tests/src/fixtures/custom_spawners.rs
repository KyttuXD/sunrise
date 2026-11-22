mod api {
    pub struct CustomSpawners {
        spawners: Vec<Box<dyn Send>>,
    }

    impl CustomSpawners {
        pub fn checkpoint(&self) -> usize {
            self.spawners.len()
        }

        pub fn rollback(&mut self, len: usize) {
            self.spawners.truncate(len);
        }
    }
}

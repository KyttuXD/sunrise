mod api {
    pub struct CustomSpawners<'a> {
        _spawners: &'a mut CustomSpawnerList,
    }

    pub struct CustomSpawnerList;

    impl CustomSpawnerList {
        pub(crate) fn edit(&mut self, _: &'static str, listener: impl FnOnce(CustomSpawners<'_>)) {
            listener(CustomSpawners { _spawners: self });
        }
    }
}

pub mod pb {
    pub mod foo {
        #[derive(Clone, PartialEq, prost::Message)]
        pub struct Message {
            #[prost(uint64, tag = "1")]
            pub value: u64,
        }

        impl prost::Name for Message {
            const NAME: &'static str = "Message";
            const PACKAGE: &'static str = "foo";

            fn full_name() -> ::alloc::string::String { "foo.Message".into() }

            fn type_url() -> ::alloc::string::String { "/foo.Message".into() }
        }
    }
}

crate::define_message! {
    pub use pb::foo::Message;
    test_message Self { value: 42 };
}

#[test]
fn test_macro() {
    // Check that test_foo test has been defined.
    #[allow(dead_code)]
    if false {
        test_message();
    }
    let msg: Message = Message::test();
    assert_eq!(42, msg.value);
}

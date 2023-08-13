//! Random stress tests.  They generate random data and perform round-trip
//! conversion checking if they result in the same output.
//!
//! The test may be slow, especially when run under MIRI.  Number of iterations
//! it performs can be controlled by STRESS_TEST_ITERATIONS environment
//! variable.

use pretty_assertions::assert_eq;

use crate::memory::Ptr;
use crate::nodes::{self, Node, NodeRef, RawNode, Reference, ValueRef};
use crate::test_utils::get_iteration_count;
use crate::{bits, stdx};

/// Generates random raw representation and checks decode→encode round-trip.
#[test]
fn stress_test_raw_encoding_round_trip() {
    let mut rng = rand::thread_rng();
    let mut raw = RawNode([0; 72]);
    for _ in 0..get_iteration_count() {
        gen_random_raw_node(&mut rng, &mut raw.0);
        let node = Node::from(&raw);

        // Test RawNode→Node→RawNode round trip conversion.
        assert_eq!(Ok(raw), RawNode::try_from(node), "node: {node:?}");
    }
}

/// Generates a random raw node representation in canonical representation.
fn gen_random_raw_node(rng: &mut impl rand::Rng, bytes: &mut [u8; 72]) {
    fn make_ref_canonical(bytes: &mut [u8]) {
        if bytes[0] & 0x40 == 0 {
            // Node reference.  Pointer can be non-zero.
            bytes[0] &= !0x80;
        } else {
            // Value reference.  Pointer must be zero but key is_sealed flag:
            // 0b01s0_0000
            bytes[..4].copy_from_slice(&0x6000_0000_u32.to_be_bytes());
        }
    }

    rng.fill(&mut bytes[..]);
    let tag = bytes[0] >> 6;
    if tag == 0 || tag == 1 {
        // Branch.
        make_ref_canonical(&mut bytes[..36]);
        make_ref_canonical(&mut bytes[36..]);
    } else if tag == 2 {
        // Extension.  Key must be valid and the most significant bit of
        // the child must be zero.  For the former it’s easiest to just
        // regenerate random data.

        // Random length and offset for the key.
        let offset = rng.gen::<u8>() % 8;
        let max_length = (nodes::MAX_EXTENSION_KEY_SIZE * 8) as u16;
        let length = rng.gen_range(1..=max_length - u16::from(offset));
        let tag = 0x8000 | (length << 3) | u16::from(offset);
        bytes[..2].copy_from_slice(&tag.to_be_bytes()[..]);

        // Clear unused bits in the key.  The easiest way to do it is by using
        // bits::Slice.
        let mut tmp = [0; 36];
        bits::Slice::new(&bytes[2..36], offset, length)
            .unwrap()
            .encode_into(&mut tmp)
            .unwrap();
        bytes[0..36].copy_from_slice(&tmp);

        make_ref_canonical(&mut bytes[36..]);
    } else {
        // Value.  Most bits in the first four bytes must be zero and child must
        // be a node reference.
        bytes[0] &= 0xE0;
        bytes[1] = 0;
        bytes[2] = 0;
        bytes[3] = 0;
        bytes[36] &= !0xC0;
    }
}

// =============================================================================

/// Generates random node and tests encode→decode round trips.
#[test]
fn stress_test_node_encoding_round_trip() {
    let mut rng = rand::thread_rng();
    let mut buf = [0; 66];
    for _ in 0..get_iteration_count() {
        let node = gen_random_node(&mut rng, &mut buf);

        let raw = super::tests::raw_from_node(&node);
        assert_eq!(node, Node::from(&raw), "Failed decoding Raw: {raw:?}");
    }
}

/// Generates a random Node.
fn gen_random_node<'a>(
    rng: &mut impl rand::Rng,
    buf: &'a mut [u8; 66],
) -> Node<'a> {
    fn rand_ref<'a>(
        rng: &mut impl rand::Rng,
        hash: &'a [u8; 32],
    ) -> Reference<'a> {
        let num = rng.gen::<u32>();
        if num < 0x8000_0000 {
            Reference::node(Ptr::new(num).ok().flatten(), hash.into())
        } else {
            Reference::value(num & 1 != 0, hash.into())
        }
    }

    rng.fill(&mut buf[..]);
    let (key, right) = stdx::split_array_ref::<34, 32, 66>(buf);
    let (_, left) = stdx::split_array_ref::<2, 32, 34>(key);
    match rng.gen_range(0..3) {
        0 => Node::branch(rand_ref(rng, &left), rand_ref(rng, &right)),
        1 => {
            let offset = rng.gen::<u8>() % 8;
            let max_length = (nodes::MAX_EXTENSION_KEY_SIZE * 8) as u16;
            let length = rng.gen_range(1..=max_length - u16::from(offset));
            let key = bits::Slice::new(&key[..], offset, length).unwrap();
            Node::extension(key, rand_ref(rng, &right))
        }
        2 => {
            let num = rng.gen::<u32>();
            let is_sealed = num & 0x8000_0000 != 0;
            let value = ValueRef::new(is_sealed, left.into());
            let ptr = Ptr::new(num & 0x7FFF_FFFF).ok().flatten();
            let child = NodeRef::new(ptr, right.into());
            Node::value(value, child)
        }
        _ => unreachable!(),
    }
}

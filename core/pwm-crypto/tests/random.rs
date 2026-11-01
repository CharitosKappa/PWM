//! Uniform random index from the OS CSPRNG (decision D21).

use pwm_crypto::random_index;

#[test]
fn stays_below_the_bound() {
    for bound in [1u32, 2, 3, 7, 10, 7776, u32::MAX] {
        for _ in 0..200 {
            assert!(random_index(bound).unwrap() < bound);
        }
    }
    assert_eq!(random_index(1).unwrap(), 0);
}

#[test]
fn is_roughly_uniform() {
    // 6 buckets x 10,000 expected; 5% is more than 5 standard deviations.
    let mut counts = [0u32; 6];
    for _ in 0..60_000 {
        counts[random_index(6).unwrap() as usize] += 1;
    }
    for c in counts {
        assert!((9_500..=10_500).contains(&c), "{counts:?}");
    }
}

#[test]
fn a_bound_just_above_half_the_range_is_unbiased() {
    // With modulo reduction, 0..2^31-1 would come up twice as often as the
    // top values; rejection sampling keeps both halves equal.
    let bound = (1u32 << 31) + (1 << 30);
    let low = (0..20_000)
        .filter(|_| random_index(bound).unwrap() < bound / 2)
        .count();
    assert!((9_300..=10_700).contains(&low), "{low}");
}

#[test]
#[should_panic(expected = "bound")]
fn a_zero_bound_panics() {
    let _ = random_index(0);
}

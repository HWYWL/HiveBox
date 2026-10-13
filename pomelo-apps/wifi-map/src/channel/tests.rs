use super::*;

fn ap(ssid: &str, rssi: i8, channel: u8) -> ApInfo {
    ApInfo::new(ssid, rssi, true, channel)
}

/// The chart is the thirteen channels of the band, in order.
#[test]
fn the_chart_is_the_thirteen_channels_of_the_band() {
    let spectrum = Spectrum::of(&[]);
    let bins = spectrum.bins();

    assert_eq!(bins.len(), 13);
    assert_eq!(bins.first().map(|bin| bin.channel), Some(FIRST));
    assert_eq!(bins.last().map(|bin| bin.channel), Some(LAST));
    assert!(bins.iter().all(|bin| in_band(bin.channel)));
}

/// A network takes all of its own channel, most of the two beside it, and none of a channel five away.
#[test]
fn an_overlap_fades_by_five_channels() {
    assert_eq!(overlap(6, 6), 1.0, "its own channel, all of it");
    assert_eq!(overlap(6, 5), 0.75);
    assert_eq!(overlap(6, 7), 0.75, "either side of it, the same");
    assert_eq!(overlap(6, 4), 0.5);
    assert_eq!(overlap(6, 3), 0.25);
    assert_eq!(overlap(6, 2), 0.1);
    assert_eq!(overlap(6, 1), 0.0, "five away is a different channel");
    assert_eq!(overlap(1, 6), 0.0, "and it is symmetric");
}

/// Symmetric, never rising with distance, never outside nought to one — the three properties the
/// arithmetic above leans on.
#[test]
fn the_overlap_only_falls_away() {
    for near in channels() {
        let mut last = 1.0;

        for far in channels() {
            let factor = overlap(near, far);

            assert!((0.0..=1.0).contains(&factor), "{near} → {far}");
            assert_eq!(factor, overlap(far, near), "{near} ↔ {far}");

            if far > near {
                assert!(factor <= last, "{near} → {far} rose");
                last = factor;
            }
        }
    }
}

/// A louder network weighs more, and both ends of the scale are clamped.
#[test]
fn a_louder_signal_weighs_more() {
    assert_eq!(weight(-35), 1.0);
    assert_eq!(weight(-20), 1.0, "clamped at the loud end");
    assert!(weight(-40) < 1.0, "and only above the ceiling is it clamped");
    assert_eq!(weight(-65), 0.5);
    assert_eq!(weight(-95), 0.0);
    assert_eq!(weight(-96), 0.0, "clamped at the quiet end");

    // Up the scale, quiet to loud, so every step is a step up: the order is the whole claim.
    let mut last = 0.0;
    for rssi in -95..=-35 {
        let now = weight(rssi as i8);

        assert!(now >= last, "{rssi} dBm weighed less than the signal below it");
        last = now;
    }
}

/// The map's rows run from the strongest signal to the weakest, ten dBm to a row.
#[test]
fn the_map_rows_are_ten_dbm_apart() {
    assert_eq!(level_of(-30), 0);
    assert_eq!(level_of(-40), 0, "the first row is everything at -40 or better");
    assert_eq!(level_of(-41), 0);
    assert_eq!(level_of(-50), 1);
    assert_eq!(level_of(-90), 5);
    assert_eq!(level_of(-100), 6, "the last row is the rest");
    assert_eq!(level_of(-127), 6, "which is where a scan's floor lands");

    let mut last = 0;
    for rssi in (-120..=-30).rev() {
        let level = level_of(rssi as i8);

        assert!(level >= last, "{rssi} dBm landed above a weaker signal");
        last = level;
    }
}

/// One network fills its own channel and spills onto its neighbours, and stops before the next
/// channel that does not overlap it.
#[test]
fn a_single_network_spills_onto_its_neighbours() {
    let spectrum = Spectrum::of(&[ap("only", -35, 6)]);

    assert_eq!(spectrum.bin(6).unwrap().pressure, 1.0);
    assert_eq!(spectrum.bin(5).unwrap().pressure, 0.75);
    assert_eq!(spectrum.bin(7).unwrap().pressure, 0.75);
    assert_eq!(spectrum.bin(4).unwrap().pressure, 0.5);
    assert_eq!(spectrum.bin(3).unwrap().pressure, 0.25);
    assert_eq!(spectrum.bin(2).unwrap().pressure, 0.1);
    assert_eq!(spectrum.bin(1).unwrap().pressure, 0.0);
    assert_eq!(spectrum.bin(11).unwrap().pressure, 0.0);

    assert_eq!(spectrum.bin(6).unwrap().aps, 1);
    assert_eq!(spectrum.bin(5).unwrap().aps, 0, "heard there, not on it");
    assert_eq!(spectrum.bin(6).unwrap().strongest, Some(-35));
    assert_eq!(spectrum.bin(5).unwrap().strongest, None);
    assert_eq!(spectrum.networks(), 1);
}

/// A quiet network spills the same way and matters less — but it is not absent.
#[test]
fn a_quiet_network_is_worth_less_than_a_loud_one() {
    let loud = Spectrum::of(&[ap("loud", -40, 6)]);
    let quiet = Spectrum::of(&[ap("quiet", -80, 6)]);

    assert!(quiet.bin(6).unwrap().pressure < loud.bin(6).unwrap().pressure);
    assert!(quiet.bin(5).unwrap().pressure < loud.bin(5).unwrap().pressure);
    assert!(quiet.crowded(), "quiet is not the same as absent");
}

/// With one loud network on 6, the answer is 1 — and 5, which is emptier, is not it.
#[test]
fn the_recommendation_avoids_the_channel_a_loud_network_is_on() {
    let spectrum = Spectrum::of(&[ap("Pomelo-OS", -35, 6)]);
    let recommended = spectrum.recommended();

    assert!(CLEAN.contains(&recommended), "{recommended}");
    assert_eq!(recommended, 1, "and the lower of the two that tie");
    assert_ne!(recommended, 5, "which is emptier but overlaps both sides");
}

/// When the quiet one of the three is taken, the recommendation moves to the one that is left.
#[test]
fn the_recommendation_moves_to_the_channel_that_is_left() {
    let spectrum = Spectrum::of(&[ap("one", -40, 1), ap("six", -40, 6)]);

    assert_eq!(spectrum.recommended(), 11);
}

/// Nothing on the air is not a special case: there is still an answer and nothing to warn about.
#[test]
fn an_empty_band_still_has_an_answer() {
    let spectrum = Spectrum::of(&[]);

    assert_eq!(spectrum.recommended(), 1);
    assert!(!spectrum.crowded());
    assert_eq!(spectrum.busiest(), None);
    assert_eq!(spectrum.peak(), 0.0);
    assert_eq!(spectrum.networks(), 0);
}

/// A pile of loud networks on one channel saturates its column instead of overfilling it, and the
/// numbers the ranking is made from are still the real ones.
#[test]
fn a_crowded_channel_saturates_its_column() {
    let spectrum = Spectrum::of(&[ap("a", -35, 6), ap("b", -40, 6), ap("c", -45, 6)]);
    let six = spectrum.bin(6).unwrap();

    assert_eq!(six.aps, 3);
    assert!(six.pressure > 2.0, "three networks, added up");
    assert_eq!(spectrum.level(six), 1.0, "drawn full, not overflowing");
    assert_eq!(spectrum.busiest().map(|bin| bin.channel), Some(6));
    assert_eq!(spectrum.recommended(), 1, "6 is still not recommended");
}

/// The busiest channel is the one under the most pressure, which is not the one with the most networks.
#[test]
fn the_busiest_channel_is_the_one_under_the_most_pressure() {
    // Two networks on 11, both far away; one loud network on 6. 11 has more networks, 6 has more noise.
    let spectrum = Spectrum::of(&[ap("far", -88, 11), ap("far too", -90, 11), ap("loud", -40, 6)]);

    assert_eq!(spectrum.bin(11).unwrap().aps, 2);
    assert_eq!(spectrum.busiest().map(|bin| bin.channel), Some(6));

    // And a tie goes to the lower channel, so the answer is not the driver's scan order.
    let tie = Spectrum::of(&[ap("a", -60, 9), ap("b", -60, 10)]);
    assert_eq!(tie.busiest().map(|bin| bin.channel), Some(9));
}

/// 5 GHz is counted and listed but kept out of the arithmetic: it takes up none of the charted band.
#[test]
fn five_gigahertz_networks_are_listed_but_not_charted() {
    let spectrum = Spectrum::of(&[ap("fast", -40, 36), ap("fast too", -55, 149), ap("slow", -70, 6)]);

    assert_eq!(spectrum.others().len(), 2);
    assert_eq!(spectrum.networks(), 3, "all three were found");
    assert_eq!(spectrum.bin(1).unwrap().pressure, 0.0);
    assert_eq!(spectrum.bin(6).unwrap().pressure, weight(-70));
    assert_eq!(spectrum.peak(), weight(-70), "the loudest was 5 GHz");

    let others: Vec<&str> = spectrum.others().iter().map(|ap| ap.ssid.as_str()).collect();
    assert_eq!(others, ["fast", "fast too"], "in the order they were found");
}

/// The 14th channel is counted but has no column, exactly like 5 GHz.
#[test]
fn the_fourteenth_channel_is_out_of_band() {
    let spectrum = Spectrum::of(&[ap("japan", -40, 14)]);

    assert!(!in_band(14));
    assert_eq!(spectrum.others().len(), 1);
    assert_eq!(spectrum.peak(), 0.0);
    assert_eq!(spectrum.recommended(), 1, "and the band still looks empty");
}

/// The loudest-first list is the band's networks, in the order a person reads them.
#[test]
fn the_list_runs_from_the_loudest_network_down() {
    let aps = [ap("b", -70, 1), ap("c", -40, 6), ap("a", -55, 11), ap("five", -30, 36)];
    let sorted = Spectrum::of(&aps).loudest(&aps);

    assert_eq!(
        sorted.iter().map(|ap| ap.ssid.as_str()).collect::<Vec<_>>(),
        ["c", "a", "b"],
        "loudest first, and the 5 GHz one is not in the band"
    );

    // Two networks at the same strength are ordered by name, so the list does not reshuffle itself
    // between two scans that found the same air.
    let tied = [ap("zebra", -60, 1), ap("alpha", -60, 1)];
    let sorted = Spectrum::of(&tied).loudest(&tied);
    assert_eq!(
        sorted.iter().map(|ap| ap.ssid.as_str()).collect::<Vec<_>>(),
        ["alpha", "zebra"]
    );
}

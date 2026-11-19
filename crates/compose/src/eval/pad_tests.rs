//! Display padding next to tiles that hold no covered texel

use super::*;

const SIDE: u32 = 128;

/// How far display padding reaches in these tests, in texels
const REACH: u32 = 4;

/// One step of 16-bit storage
const STORAGE_STEP: f32 = 1.0 / 65535.0;

/// The stored components of a channel's default value
fn stored_default(channel: Channel) -> Vec<u16> {
    let encode = |value: f32| if channel.kind() == ChannelKind::Signed { to_u16_signed(value) } else { to_u16(value) };
    channel.default_value().iter().take(channel.components()).map(|value| encode(*value)).collect()
}

#[test]
fn a_tile_that_padding_allocates_starts_at_each_channels_default() {
    // One covered texel two texels from the border between tile (0, 0) and the empty tile (1, 0)
    let (island_x, island_y) = (TILE - 2, 10);
    let mut covered = vec![false; (SIDE * SIDE) as usize];
    covered[(island_y * SIDE + island_x) as usize] = true;
    let dilation = DilationMap::new(&covered, SIDE, SIDE, Some(REACH));

    let mut out = Composite::new(&Channel::ALL, SIDE);
    let painted: Vec<u16> = vec![51000, 12000, 40000];
    for (channel, image) in &mut out.channels {
        image.set(island_x, island_y, &painted[..channel.components()]);
    }

    let written = pad_tiles(&mut out, &dilation, &[(0, 0)]);
    assert!(written.contains(&(1, 0)), "padding reaches into the next tile: {written:?}");

    for channel in Channel::ALL {
        let image = &out.channels[&channel];
        let components = channel.components();
        assert!(image.is_allocated(1, 0), "{channel:?}");

        // The strip the padding reached holds the island's value
        assert_eq!(image.get(TILE + 1, island_y)[..components], painted[..components], "{channel:?}: padded texel");

        // The rest of the tile reads as an untouched tile does: the channel's default
        for (x, y) in [(TILE + REACH + 1, island_y), (TILE + 30, 40), (2 * TILE - 1, TILE - 1)] {
            assert_eq!(image.get(x, y)[..components], stored_default(channel)[..], "{channel:?} at ({x}, {y})");

            // To within the storage step, since a default such as 0.5 is not a stored level
            let (allocated, untouched) = (out.value(channel, x, y), out.value(channel, x, TILE + y));
            assert!(!out.channels[&channel].is_allocated(1, 1) && untouched == channel.default_value());
            assert!(allocated.iter().zip(untouched).all(|(a, b)| (a - b).abs() <= STORAGE_STEP), "{channel:?}: {allocated:?} against {untouched:?}");
        }
    }
}

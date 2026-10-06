//! Row-band parallelism for CPU rasterisation.

use std::sync::Mutex;

const BYTES_PER_PIXEL: usize = 4;

pub(crate) fn for_each_row_band(
    destination: &mut [u8],
    destination_size: [u32; 2],
    min_y: i32,
    max_y: i32,
    paint_row: impl Fn(i32, &mut [u8]) + Sync,
) {
    let row_bytes = destination_size[0] as usize * BYTES_PER_PIXEL;
    for_each_band(
        destination,
        destination_size,
        min_y,
        max_y,
        |band_start_y, band| {
            for (offset, row) in band.chunks_exact_mut(row_bytes).enumerate() {
                paint_row(band_start_y + offset as i32, row);
            }
        },
    );
}

pub(crate) fn for_each_band(
    destination: &mut [u8],
    destination_size: [u32; 2],
    min_y: i32,
    max_y: i32,
    paint_band: impl Fn(i32, &mut [u8]) + Sync,
) {
    let start_y = min_y.max(0) as usize;
    let end_y = (max_y.saturating_add(1)).clamp(0, destination_size[1] as i32) as usize;
    if start_y >= end_y {
        return;
    }
    let row_bytes = destination_size[0] as usize * BYTES_PER_PIXEL;
    let rows = &mut destination[start_y * row_bytes..end_y * row_bytes];
    let row_count = end_y - start_y;
    let workers = std::thread::available_parallelism()
        .map_or(1, usize::from)
        .min(row_count / 32)
        .max(1);
    if workers <= 1 {
        paint_band(start_y as i32, rows);
        return;
    }
    let rows_per_band = row_count.div_ceil(workers * 8).max(4);
    let bands = Mutex::new(rows.chunks_mut(rows_per_band * row_bytes).enumerate());
    let paint_bands = || {
        loop {
            let Some((band_index, band)) = bands.lock().map_or(None, |mut bands| bands.next())
            else {
                return;
            };
            paint_band((start_y + band_index * rows_per_band) as i32, band);
        }
    };
    std::thread::scope(|scope| {
        for _ in 1..workers {
            scope.spawn(paint_bands);
        }
        paint_bands();
    });
}

#[cfg(test)]
mod tests {
    use super::{for_each_band, for_each_row_band};

    #[test]
    fn row_bands_paint_each_requested_row_once_like_a_serial_loop() {
        let size = [7_u32, 300];
        for (min_y, max_y) in [(0, 299), (13, 250), (-5, 400), (40, 40), (90, 10)] {
            let mut banded = vec![0_u8; 7 * 300 * 4];
            for_each_row_band(&mut banded, size, min_y, max_y, |y, row| {
                for (x, pixel) in row.chunks_exact_mut(4).enumerate() {
                    pixel[0] = pixel[0].wrapping_add(1);
                    pixel[1] = y as u8;
                    pixel[2] = x as u8;
                }
            });
            let mut serial = vec![0_u8; 7 * 300 * 4];
            for y in min_y.max(0)..=max_y.min(299) {
                for x in 0..7 {
                    let index = (y as usize * 7 + x) * 4;
                    serial[index..index + 3].copy_from_slice(&[1, y as u8, x as u8]);
                }
            }
            assert_eq!(banded, serial, "rows {min_y}..={max_y}");
        }
    }

    #[test]
    fn bands_cover_each_requested_row_once_starting_at_their_first_row() {
        let size = [3_u32, 1000];
        let mut seen = vec![0_u8; 3 * 1000 * 4];
        for_each_band(&mut seen, size, 10, 989, |start_y, band| {
            for (offset, row) in band.chunks_exact_mut(12).enumerate() {
                row[0] += 1;
                row[1] = (start_y as usize + offset) as u8;
            }
        });
        for y in 0..1000 {
            let row = &seen[y * 12..y * 12 + 2];
            let expected = if (10..=989).contains(&y) {
                [1, y as u8]
            } else {
                [0, 0]
            };
            assert_eq!(row, expected, "row {y}");
        }
    }
}

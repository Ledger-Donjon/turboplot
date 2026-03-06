//! Tektronix WFM file format parser (versions 1, 2, 3).
//!
//! Reference: Tektronix "Reference Waveform File Format" manual (077-0220-11)

use std::io::Read;

/// WFM file format version.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WfmVersion {
    V1,
    V2,
    V3,
}

/// Curve data encoding format (explicit dimension).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ExplicitFormat {
    Int16,
    Int32,
    Uint32,
    Uint64,
    Fp32,
    Fp64,
    Uint8,
    Int8,
}

impl ExplicitFormat {
    /// Returns the number of bytes for one data point.
    fn bytes_per_point(self) -> usize {
        match self {
            Self::Int8 | Self::Uint8 => 1,
            Self::Int16 => 2,
            Self::Int32 | Self::Uint32 | Self::Fp32 => 4,
            Self::Uint64 | Self::Fp64 => 8,
        }
    }
}

/// Binary parser with configurable byte order.
struct WfmParser {
    data: Vec<u8>,
    pos: usize,
    little_endian: bool,
}

impl WfmParser {
    fn new(data: Vec<u8>) -> Self {
        Self {
            data,
            pos: 0,
            little_endian: true,
        }
    }

    fn skip(&mut self, n: usize) {
        self.pos += n;
    }

    fn read_u8(&mut self) -> u8 {
        let v = self.data[self.pos];
        self.pos += 1;
        v
    }

    fn read_u32(&mut self) -> u32 {
        let bytes: [u8; 4] = self.data[self.pos..self.pos + 4].try_into().unwrap();
        self.pos += 4;
        if self.little_endian {
            u32::from_le_bytes(bytes)
        } else {
            u32::from_be_bytes(bytes)
        }
    }

    fn read_f64(&mut self) -> f64 {
        let bytes: [u8; 8] = self.data[self.pos..self.pos + 8].try_into().unwrap();
        self.pos += 8;
        if self.little_endian {
            f64::from_le_bytes(bytes)
        } else {
            f64::from_be_bytes(bytes)
        }
    }

    fn read_string(&mut self, n: usize) -> String {
        let bytes = &self.data[self.pos..self.pos + n];
        self.pos += n;
        let end = bytes.iter().position(|&b| b == 0).unwrap_or(n);
        String::from_utf8_lossy(&bytes[..end]).to_string()
    }

    /// Read a single raw curve data point at the given byte offset in the file.
    fn read_sample_at(&self, offset: usize, format: ExplicitFormat) -> f64 {
        match format {
            ExplicitFormat::Int8 => self.data[offset] as i8 as f64,
            ExplicitFormat::Uint8 => self.data[offset] as f64,
            ExplicitFormat::Int16 => {
                let bytes: [u8; 2] = self.data[offset..offset + 2].try_into().unwrap();
                if self.little_endian {
                    i16::from_le_bytes(bytes) as f64
                } else {
                    i16::from_be_bytes(bytes) as f64
                }
            }
            ExplicitFormat::Int32 => {
                let bytes: [u8; 4] = self.data[offset..offset + 4].try_into().unwrap();
                if self.little_endian {
                    i32::from_le_bytes(bytes) as f64
                } else {
                    i32::from_be_bytes(bytes) as f64
                }
            }
            ExplicitFormat::Uint32 => {
                let bytes: [u8; 4] = self.data[offset..offset + 4].try_into().unwrap();
                if self.little_endian {
                    u32::from_le_bytes(bytes) as f64
                } else {
                    u32::from_be_bytes(bytes) as f64
                }
            }
            ExplicitFormat::Uint64 => {
                let bytes: [u8; 8] = self.data[offset..offset + 8].try_into().unwrap();
                if self.little_endian {
                    u64::from_le_bytes(bytes) as f64
                } else {
                    u64::from_be_bytes(bytes) as f64
                }
            }
            ExplicitFormat::Fp32 => {
                let bytes: [u8; 4] = self.data[offset..offset + 4].try_into().unwrap();
                if self.little_endian {
                    f32::from_le_bytes(bytes) as f64
                } else {
                    f32::from_be_bytes(bytes) as f64
                }
            }
            ExplicitFormat::Fp64 => {
                let bytes: [u8; 8] = self.data[offset..offset + 8].try_into().unwrap();
                if self.little_endian {
                    f64::from_le_bytes(bytes)
                } else {
                    f64::from_be_bytes(bytes)
                }
            }
        }
    }

    /// Read a WfmCurveObject (30 bytes) and return (data_start, postcharge_start, postcharge_stop).
    /// All offsets are local to the frame's portion of the curve buffer.
    fn read_curve_object(&mut self) -> (usize, usize, usize) {
        self.skip(4 + 4 + 2); // state_flags, type_of_checksum, checksum
        let _precharge_start = self.read_u32();
        let data_start = self.read_u32() as usize;
        let postcharge_start = self.read_u32() as usize;
        let postcharge_stop = self.read_u32() as usize;
        self.skip(4); // end_of_curve
        (data_start, postcharge_start, postcharge_stop)
    }

    /// Skip an explicit dimension's user view data section.
    fn skip_user_view(&mut self, version: WfmVersion) {
        self.skip(8); // user_scale
        self.skip(20); // user_units
        self.skip(8); // user_offset
        if version == WfmVersion::V3 {
            self.skip(8); // point_density (f64 in V3)
        } else {
            self.skip(4); // point_density (u32 in V1/V2)
        }
        self.skip(8); // href
        self.skip(8); // trig_delay
    }
}

/// Loads a Tektronix WFM file and returns all frames as separate traces.
///
/// For single-frame files, returns a `Vec` with one element.
/// For FastFrame files, returns one trace per frame.
///
/// Raw curve data is converted using: `voltage = raw_value * scale + offset`
/// where scale and offset come from the explicit dimension 1 header.
pub fn load_tek_wfm<R: Read>(mut reader: R, path: &str) -> Vec<Vec<f32>> {
    let mut data = Vec::new();
    reader
        .read_to_end(&mut data)
        .expect("Failed to read WFM file");
    assert!(
        data.len() >= 78,
        "WFM file too small for static file header"
    );

    let mut p = WfmParser::new(data);

    // ==== Static file information (78 bytes) ====

    // Byte order verification (2 bytes).
    let byte_order_raw = u16::from_le_bytes([p.data[0], p.data[1]]);
    p.little_endian = match byte_order_raw {
        0x0F0F => true,
        0xF0F0 => false,
        _ => panic!("Invalid WFM byte order: 0x{:04X}", byte_order_raw),
    };
    p.pos = 2;

    // Version string (8 bytes)
    let version_str = p.read_string(8);
    let version = if version_str.contains("WFM#001") {
        WfmVersion::V1
    } else if version_str.contains("WFM#002") {
        WfmVersion::V2
    } else if version_str.contains("WFM#003") {
        WfmVersion::V3
    } else {
        panic!("Unsupported WFM version: {}", version_str);
    };

    p.skip(1 + 4); // num_digits_in_byte_count, bytes_to_eof
    let bytes_per_point = p.read_u8() as usize;
    let curve_buffer_offset = p.read_u32() as usize;
    p.skip(4 + 4 + 8 + 4); // hz_zoom_scale, hz_zoom_pos, vt_zoom_scale, vt_zoom_pos
    p.skip(32); // waveform_label
    let n_fast_frames_minus_one = p.read_u32();
    p.skip(2); // wfm_header_size

    // ==== Waveform header ====
    // set_type(4) + wfm_cnt(4) + acq_counter(8) + trans_counter(8) + slot_id(4) +
    // is_static(4) + update_spec_count(4) + imp_dim_ref_count(4) + exp_dim_ref_count(4)
    p.skip(4 + 4 + 8 + 8 + 4 + 4 + 4 + 4 + 4);
    let data_type = p.read_u32(); // data_type
    // gen_purpose_counter(8) + accum_count(4) + target_accum(4) + curve_ref_count(4) +
    // num_req_ff(4) + num_acq_ff(4)
    p.skip(8 + 4 + 4 + 4 + 4 + 4);
    if version != WfmVersion::V1 {
        p.skip(2); // summary_frame (V2/V3 only)
    }
    p.skip(4 + 8); // pix_map_display_format, pix_map_max_value

    // ==== Explicit Dimension 1 (voltage axis) ====
    let exp_dim1_scale = p.read_f64();
    let exp_dim1_offset = p.read_f64();
    p.skip(4 + 20); // dim_size, units
    p.skip(8 + 8 + 8 + 8); // extent_min, extent_max, resolution, ref_point
    let exp_dim1_format_raw = p.read_u32();
    p.skip(4 + 4 * 5); // storage_type, n_value, over_range, under_range, high_range, low_range
    p.skip_user_view(version);

    // ==== Explicit Dimension 2 (skip entirely: 100 bytes description + user view) ====
    p.skip(8 + 8 + 4 + 20 + 8 + 8 + 8 + 8 + 4 + 4 + 4 * 5);
    p.skip_user_view(version);

    // ==== Implicit Dimension 1 (time axis) ====
    let imp_dim1_scale = p.read_f64();
    // offset(8) + size(4) + units(20) + extent_min(8) + extent_max(8) +
    // resolution(8) + ref_point(8) + spacing(4)
    p.skip(8 + 4 + 20 + 8 + 8 + 8 + 8 + 4);
    p.skip_user_view(version);

    // ==== Implicit Dimension 2 (skip entirely: 76 bytes description + user view) ====
    p.skip(8 + 8 + 4 + 20 + 8 + 8 + 8 + 8 + 4);
    p.skip_user_view(version);

    // ==== Time Base 1 & 2 (12 bytes each) ====
    p.skip(12 + 12);

    // ==== WfmUpdateSpec (first frame, 24 bytes) ====
    p.skip(24); // real_point_offset(4) + tt_offset(8) + frac_sec(8) + gmt_sec(4)

    // ==== WfmCurveObject (first frame, 30 bytes) ====
    let mut frame_offsets = vec![p.read_curve_object()];

    // ==== FastFrame additional frames ====
    if n_fast_frames_minus_one > 0 {
        let n = n_fast_frames_minus_one as usize;
        p.skip(n * 24); // N-1 additional WfmUpdateSpecs
        for _ in 0..n {
            frame_offsets.push(p.read_curve_object());
        }
    }

    // ==== Determine data format ====

    let format = match (exp_dim1_format_raw, version) {
        (0, _) => ExplicitFormat::Int16,
        (1, _) => ExplicitFormat::Int32,
        (2, _) => ExplicitFormat::Uint32,
        (3, _) => ExplicitFormat::Uint64,
        (4, _) => ExplicitFormat::Fp32,
        (5, _) => ExplicitFormat::Fp64,
        (6, WfmVersion::V2 | WfmVersion::V3) => ExplicitFormat::Uint8,
        (7, WfmVersion::V2 | WfmVersion::V3) => ExplicitFormat::Int8,
        _ => panic!(
            "Unsupported explicit format {} for version {:?}",
            exp_dim1_format_raw, version
        ),
    };

    assert_eq!(
        bytes_per_point,
        format.bytes_per_point(),
        "Bytes per point mismatch: header says {} but format {:?} requires {}",
        bytes_per_point,
        format,
        format.bytes_per_point()
    );

    if data_type == 5 {
        println!("Warning: Waveform database format. May not display as a simple trace.");
    }

    // ==== Read curve data for each frame ====
    // Curve offsets in each WfmCurveObject are LOCAL to that frame's portion of the
    // contiguous curve buffer.  The frame stride (postcharge_stop from frame 0) gives
    // the size of each frame's region inside the buffer.

    let total_frames = frame_offsets.len();
    let frame_stride = frame_offsets[0].2;
    let mut all_frames = Vec::with_capacity(total_frames);

    for (frame_idx, &(data_start, postcharge_start, _)) in frame_offsets.iter().enumerate() {
        let frame_base = curve_buffer_offset + frame_idx * frame_stride;
        let curve_data_start = frame_base + data_start;
        let curve_data_end = frame_base + postcharge_start;

        assert!(
            curve_data_end <= p.data.len(),
            "Frame {} curve data extends beyond file: end offset {} > file size {}",
            frame_idx,
            curve_data_end,
            p.data.len()
        );

        let num_points = (curve_data_end - curve_data_start) / bytes_per_point;

        let mut samples = Vec::with_capacity(num_points);
        for i in 0..num_points {
            let offset = curve_data_start + i * bytes_per_point;
            let raw = p.read_sample_at(offset, format);
            samples.push((raw * exp_dim1_scale + exp_dim1_offset) as f32);
        }
        all_frames.push(samples);
    }

    let sampling_rate = if imp_dim1_scale > 0.0 {
        1.0 / imp_dim1_scale
    } else {
        f64::NAN
    };
    let pts = if let Some(first) = all_frames.first() {
        first.len()
    } else {
        0
    };
    println!(
        "{}: Tektronix WFM {:?}, {:?}, {:.3} MS/s, {} frame(s), {} pts/frame",
        path,
        version,
        format,
        sampling_rate / 1e6,
        total_frames,
        pts
    );

    all_frames
}

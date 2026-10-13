//! DCD trajectory output for docking pose series (#276).
//!
//! A minimal, dependency-free CHARMM-style DCD writer: the format is four
//! Fortran records plus one record per coordinate axis per frame, which is
//! simple enough to emit without an xdrfile-style C dependency (the reason
//! DCD was chosen over XTC for the docking batch). The viewer's existing
//! Mol* trajectory pipeline replays these files with its stock animation
//! controls.
//!
//! Layout (little-endian):
//!
//! ```text
//! int32 84 | "CORD" | int32[20] icntrl | int32 84      header
//! int32 n  | title bytes                | int32 n      title
//! int32 4  | int32 natoms               | int32 4      atom count
//! per frame, three times (x, y, z):
//!   int32 4*natoms | float32[natoms]    | int32 4*natoms
//! ```
//!
//! `icntrl[0]` (frame count) is back-filled after the last frame so the
//! writer streams every frame without buffering the file in memory.

use std::io::{BufWriter, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

/// Write `frames` as a CHARMM-style DCD. Every frame must carry the same
/// atom count; the file is streamed frame by frame and the frame count in
/// the header is patched afterwards, so thousands of frames do not
/// allocate proportional writer-side memory.
pub fn write_dcd(path: &Path, frames: &[Vec<[f64; 3]>]) -> Result<(), String> {
    let natoms = frames.first().map(|frame| frame.len()).unwrap_or(0);
    if natoms == 0 {
        return Err("cannot write a DCD with zero atoms".into());
    }
    if frames.iter().any(|frame| frame.len() != natoms) {
        return Err("all DCD frames must have the same atom count".into());
    }
    let mut writer =
        BufWriter::new(std::fs::File::create(path).map_err(|error| error.to_string())?);
    let i32_le = |writer: &mut BufWriter<std::fs::File>, value: i32| {
        writer
            .write_all(&value.to_le_bytes())
            .map_err(|e| e.to_string())
    };

    // Header record: marker, "CORD", 20 control ints, marker.
    i32_le(&mut writer, 84)?;
    writer.write_all(b"CORD").map_err(|e| e.to_string())?;
    let mut icntrl = [0i32; 20];
    icntrl[0] = frames.len() as i32; // patched again after the last frame
    icntrl[1] = 1; // first step
    icntrl[2] = 1; // steps between saves
    icntrl[12] = 24; // CHARMM version marker, no extra/4D blocks follow
    for value in icntrl {
        i32_le(&mut writer, value)?;
    }
    i32_le(&mut writer, 84)?;

    // Title record.
    let title = "wisp-science docking poses";
    i32_le(&mut writer, title.len() as i32)?;
    writer
        .write_all(title.as_bytes())
        .map_err(|e| e.to_string())?;
    i32_le(&mut writer, title.len() as i32)?;

    // Atom-count record.
    i32_le(&mut writer, 4)?;
    i32_le(&mut writer, natoms as i32)?;
    i32_le(&mut writer, 4)?;

    // Frames: one Fortran record per axis, streamed.
    let record_bytes = (4 * natoms) as i32;
    for frame in frames {
        for axis in 0..3 {
            i32_le(&mut writer, record_bytes)?;
            let mut block = Vec::with_capacity(4 * natoms);
            for position in frame {
                block.extend_from_slice(&(position[axis] as f32).to_le_bytes());
            }
            writer.write_all(&block).map_err(|e| e.to_string())?;
            i32_le(&mut writer, record_bytes)?;
        }
    }
    // Back-fill the frame count now that everything is on disk.
    writer.flush().map_err(|e| e.to_string())?;
    let mut file = writer.into_inner().map_err(|e| e.to_string())?;
    file.seek(SeekFrom::Start(8)).map_err(|e| e.to_string())?;
    file.write_all(&(frames.len() as i32).to_le_bytes())
        .map_err(|e| e.to_string())?;
    file.flush().map_err(|e| e.to_string())
}

/// Collect the `ATOM`/`HETATM` coordinates of each pose frame (as written
/// by [`crate::docking::dock_builtin`]) and write them as one DCD whose
/// first pose file doubles as the topology. Frames follow the given order,
/// i.e. rank order for docking results. Returns the atom count per frame.
pub fn write_dcd_from_pose_files(pose_paths: &[PathBuf], out_path: &Path) -> Result<usize, String> {
    if pose_paths.is_empty() {
        return Err("no pose frames to animate".into());
    }
    let mut frames: Vec<Vec<[f64; 3]>> = Vec::with_capacity(pose_paths.len());
    for path in pose_paths {
        frames.push(pdb_coordinates(path)?);
    }
    let natoms = frames[0].len();
    if natoms == 0 {
        return Err("pose frames contain no ATOM/HETATM records".into());
    }
    write_dcd(out_path, &frames)?;
    Ok(natoms)
}

/// Minimal ATOM/HETATM coordinate extraction in the fixed PDB columns.
fn pdb_coordinates(path: &Path) -> Result<Vec<[f64; 3]>, String> {
    let text = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
    let mut positions = Vec::new();
    for line in text.lines() {
        if !line.starts_with("ATOM  ") && !line.starts_with("HETATM") {
            continue;
        }
        let column = |start: usize| -> Result<f64, String> {
            line.get(start..start + 8)
                .and_then(|field| field.trim().parse::<f64>().ok())
                .ok_or_else(|| format!("malformed coordinates in {path:?}: {line}"))
        };
        positions.push([column(30)?, column(38)?, column(46)?]);
    }
    Ok(positions)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_path(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!("wisp-dcd-{}-{name}", std::process::id()))
    }

    fn sample_frames(count: usize, natoms: usize) -> Vec<Vec<[f64; 3]>> {
        (0..count)
            .map(|frame| {
                (0..natoms)
                    .map(|atom| {
                        [
                            frame as f64 + atom as f64 * 0.5,
                            (frame as f64 * 2.0) - atom as f64,
                            frame as f64 * 0.25 + atom as f64,
                        ]
                    })
                    .collect()
            })
            .collect()
    }

    fn read_i32(bytes: &[u8], offset: usize) -> i32 {
        i32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap())
    }

    fn read_f32(bytes: &[u8], offset: usize) -> f32 {
        f32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap())
    }

    #[test]
    fn dcd_header_and_frame_bytes_follow_the_charmm_layout() {
        let frames = sample_frames(2, 3);
        let path = temp_path("layout.dcd");
        write_dcd(&path, &frames).unwrap();
        let bytes = std::fs::read(&path).unwrap();
        // Header record: 4 + 4 + 80 + 4 = 92 bytes.
        assert_eq!(read_i32(&bytes, 0), 84);
        assert_eq!(&bytes[4..8], b"CORD");
        assert_eq!(read_i32(&bytes, 8), 2, "frame count");
        assert_eq!(read_i32(&bytes, 12), 1, "first step");
        assert_eq!(read_i32(&bytes, 16), 1, "save interval");
        assert_eq!(read_i32(&bytes, 56), 24, "CHARMM version at icntrl[12]");
        assert_eq!(read_i32(&bytes, 88), 84, "header close marker");
        // Title record: marker, bytes, marker.
        let title_len = read_i32(&bytes, 92) as usize;
        assert_eq!(&bytes[96..96 + title_len], b"wisp-science docking poses");
        // Atom-count record follows the title.
        let atoms_offset = 100 + title_len;
        assert_eq!(read_i32(&bytes, atoms_offset), 4);
        assert_eq!(read_i32(&bytes, atoms_offset + 4), 3, "natoms");
        assert_eq!(read_i32(&bytes, atoms_offset + 8), 4);
        // First frame: x record with a matching marker pair.
        let frame_offset = atoms_offset + 12;
        assert_eq!(read_i32(&bytes, frame_offset), 12);
        assert!((read_f32(&bytes, frame_offset + 4) - frames[0][0][0] as f32).abs() < 1e-6);
        assert!((read_f32(&bytes, frame_offset + 8) - frames[0][1][0] as f32).abs() < 1e-6);
        assert_eq!(read_i32(&bytes, frame_offset + 16), 12);
        // Size: header 92 + title + 12 + 2 frames * 3 axes * (4 + 12 + 4).
        let expected = 92 + (4 + title_len + 4) + 12 + 2 * 3 * 20;
        assert_eq!(bytes.len(), expected);
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn dcd_round_trips_every_frame_and_atom() {
        let frames = sample_frames(4, 5);
        let path = temp_path("roundtrip.dcd");
        write_dcd(&path, &frames).unwrap();
        let bytes = std::fs::read(&path).unwrap();
        // Locate the first frame (fixed title length, see the layout test).
        let title_len = read_i32(&bytes, 92) as usize;
        let mut offset = 92 + (4 + title_len + 4) + 12;
        for frame in &frames {
            for axis in 0..3 {
                offset += 4; // record marker
                for position in frame {
                    let value = read_f32(&bytes, offset);
                    assert!(
                        (value - position[axis] as f32).abs() < 1e-6,
                        "axis {axis} mismatch"
                    );
                    offset += 4;
                }
                offset += 4; // record marker
            }
        }
        assert_eq!(offset, bytes.len());
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn large_frame_counts_stream_without_error() {
        // Over a thousand frames, exercising the streaming path and the
        // back-filled frame count.
        let frames = sample_frames(1200, 4);
        let path = temp_path("large.dcd");
        write_dcd(&path, &frames).unwrap();
        let bytes = std::fs::read(&path).unwrap();
        assert_eq!(read_i32(&bytes, 8), 1200, "back-filled frame count");
        let title_len = read_i32(&bytes, 92) as usize;
        let expected = 92 + (4 + title_len + 4) + 12 + 1200 * 3 * (4 + 16 + 4);
        assert_eq!(bytes.len(), expected);
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn mismatched_atom_counts_are_rejected() {
        let frames = vec![vec![[0.0; 3]; 3], vec![[0.0; 3]; 4]];
        let path = temp_path("mismatch.dcd");
        let error = write_dcd(&path, &frames).unwrap_err();
        assert!(error.contains("same atom count"), "{error}");
        assert!(!path.exists());
    }

    #[test]
    fn pose_files_animate_into_a_single_dcd() {
        let dir = std::env::temp_dir().join(format!("wisp-dcd-poses-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let receptor =
            "ATOM      1  CA  GLY A   1       1.000   2.000   3.000  1.00  0.00           C\n";
        let mut paths = Vec::new();
        for (index, x) in [5.0_f64, 6.0, 7.0].iter().enumerate() {
            let path = dir.join(format!("pose-{:03}.pdb", index + 1));
            let body = format!(
                "{receptor}HETATM    1  C   LIG L   1   {x:8.3}   0.000   0.000  1.00  0.00           C\nEND\n"
            );
            std::fs::write(&path, body).unwrap();
            paths.push(path);
        }
        let out = dir.join("poses.dcd");
        let natoms = write_dcd_from_pose_files(&paths, &out).unwrap();
        assert_eq!(natoms, 2, "receptor + ligand atom");
        let bytes = std::fs::read(&out).unwrap();
        assert_eq!(read_i32(&bytes, 8), 3);
        // The ligand x coordinate of the last frame sits in the x block of
        // frame 3.
        let title_len = read_i32(&bytes, 92) as usize;
        let frame3 = 92 + (4 + title_len + 4) + 12 + 2 * 3 * (4 + 8 + 4);
        assert!((read_f32(&bytes, frame3 + 4 + 4) - 7.0).abs() < 1e-6);
        std::fs::remove_dir_all(&dir).ok();
    }
}

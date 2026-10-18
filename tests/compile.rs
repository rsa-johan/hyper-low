use std::path::Path;

#[test]
fn example_package_compiles() {
    let pkg = hyper_low::compile_package(Path::new("protos/examples")).unwrap();
    assert_eq!(pkg.name, "examples");
    assert!(pkg.mod_rs.contains("pub mod common_inc;"));
    assert!(pkg.mod_rs.contains("pub mod dummy;"));

    let dummy = &pkg.units.iter().find(|(m, _)| m == "dummy").unwrap().1;
    assert!(dummy.rust.contains("use super::common_inc::*;"));
    assert!(!dummy.rust.contains("pub enum NodeId"), "included items must not be duplicated");
    assert!(dummy.rust.contains("pub const RSP_ID_MATCH: u32 = 0x209;"));
    assert!(dummy.c.contains("#include \"common_inc.h\""));
    assert!(dummy.c.contains("static inline bool GetTemperature_is_response"));
}

#[test]
fn reports_line_numbers() {
    let err = hyper_low::compile_str("def {0} A:\n.x 0 bool\n.y 0 bool\n", "t", "t.cp").unwrap_err();
    assert_eq!(err.to_string(), "line 3: field `y` overlaps `x` at byte 0 bit 0");
}

#[test]
fn def_fields_need_a_type() {
    let err = hyper_low::compile_str("def {0} A:\n.x 0\n", "t", "t.cp").unwrap_err();
    assert!(err.to_string().contains("the type is required"), "{err}");
    let err = hyper_low::compile_str("def {0} A:\n.x 0..3 bool\n", "t", "t.cp").unwrap_err();
    assert!(err.to_string().contains("a 3-bit field does not fit `bool`"), "{err}");
}

#[test]
fn rejects_overlapping_filters() {
    let src = "filter A(n: u8):\n.id 0..3 n\ndef {0} D:\n.x 0 bool\n\
               frame X:\n.filter A(1)\n.D\nfn F(): D\n.rsp A(1)\n";
    let err = hyper_low::compile_str(src, "t", "t.cp").unwrap_err();
    assert!(err.to_string().contains("can match the same frame"), "{err}");
}

#[test]
fn signed_and_big_endian_fields() {
    let src = "#endian=big\ndef {0,1} R:\n.rpm 0..12 i16\n.t 4 bool little\nfn S(a: u16): Void\n";
    let g = hyper_low::compile_str(src, "t", "t.cp").unwrap();
    assert!(g.rust.contains("pub rpm: i16,"));
    assert!(g.rust.contains("let v = (buf[1] as u64) | (((buf[0] & 0x0F) as u64) << 8);"), "{}", g.rust);
    assert!(g.rust.contains("to_be_bytes"));
    assert!(g.c.contains("m->rpm = (int16_t)((v & 0x800u) ? -(int64_t)(~v & 0xFFFu) - 1 : (int64_t)v);"));
}

#[test]
fn id_type_separates_filters() {
    let src = "#id=extended\nfilter A:\n.id 0..11 1\nfilter Std:\n.ext false\ndef {0} D:\n.x 0 bool\n\
               frame X:\n.filter A\n.D\nframe Y:\n.filter A | Std\n.D\n";
    let g = hyper_low::compile_str(src, "t", "t.cp").unwrap();
    assert!(g.rust.contains("pub const FILTER_EXTENDED: Option<bool> = Some(true);"));
    assert!(g.rust.contains("pub const FILTER_EXTENDED: Option<bool> = Some(false);"));
    assert!(g.c.contains("if (extended) return false;"));

    let err = hyper_low::compile_str("#id=standard\nfilter H:\n.id 11 1\ndef {0} D:\n.x 0 bool\nframe F:\n.filter H\n.D\n", "t", "t.cp")
        .unwrap_err();
    assert!(err.to_string().contains("standard (11-bit) ids have no bit 11"), "{err}");
}

#[test]
fn generic_defs_are_placed_at_use() {
    let src = "def '2 P:\n.a .. u8\n[#1]\n.b .. u8\nframe F:\n.size '6\n.x: P {0}\n.y: P {4}\nfn S(k: u8, p: P): Void\n";
    let g = hyper_low::compile_str(src, "t", "t.cp").unwrap();
    assert!(g.rust.contains("y: P::decode_at(buf, 4)?,"), "{}", g.rust);
    assert!(g.rust.contains("self.p.write_at(buf, 1);"));
    assert!(g.c.contains("P_write_at(&m->y, buf, 4u);"));

    let err = hyper_low::compile_str("def '2 P:\n.a .. u8\nframe F:\n.x: P {0}\n.y: P {1}\n", "t", "t.cp").unwrap_err();
    assert!(err.to_string().contains("`y` and `x` both use byte 1"), "{err}");
    let err = hyper_low::compile_str("def '2 P:\n.a .. u8\nframe F:\n.P\n", "t", "t.cp").unwrap_err();
    assert!(err.to_string().contains("needs a start byte"), "{err}");
}

#[test]
fn id_ranges() {
    let src = "filter G(lo: u8, hi: u8):\n.id 8..11 lo..=hi\ndef {0} D:\n.x 0 bool\n\
               frame X:\n.filter G(1, 3)\n.D\nframe Y:\n.filter G(4, 7)\n.D\n";
    let g = hyper_low::compile_str(src, "t", "t.cp").unwrap();
    assert!(g.rust.contains("FILTER_ID_TERMS: &'static [(u32, u32)] = &[(0x700, 0x100), (0x600, 0x200)];"), "{}", g.rust);
    assert!(g.rust.contains("FILTER_ID_TERMS: &'static [(u32, u32)] = &[(0x400, 0x400)];"));
    assert!(g.c.contains("if (((id >> 8) & 0x7u) > 3u) return false;"));

    let err = hyper_low::compile_str(&src.replace("G(4, 7)", "G(3, 7)"), "t", "t.cp").unwrap_err();
    assert!(err.to_string().contains("can match the same frame"), "{err}");
    let err = hyper_low::compile_str("filter A:\n.data {0} 0..4 1..=3\n", "t", "t.cp").unwrap_err();
    assert!(err.to_string().contains("only supported on `.id`"), "{err}");
}

#[test]
fn rejects_conflicting_pipe() {
    let src = "filter A:\n.id 0 1\nfilter B:\n.id 0 0\ndef {0} D:\n.x 0 bool\nframe X:\n.filter A | B\n.D\n";
    let err = hyper_low::compile_str(src, "t", "t.cp").unwrap_err();
    assert!(err.to_string().contains("conflicting values for id bit 0"), "{err}");
}

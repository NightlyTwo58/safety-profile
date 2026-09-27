// inputs/techmap.v
//
// Yosys techmap: renames internal RTLIL cells to the CellType names that
// safety_pass::Cell::from_id / CellType::from_str expect.
//
// Usage (inside a Yosys script):
//   techmap -map inputs/techmap.v
//
// After read_verilog + proc, Yosys represents every gate as one of its
// internal primitive cells ($_NAND_, $_AND_, $_NOT_, ...). This file tells
// Yosys to replace each of those with a module instantiation whose type name
// matches a CellType variant, so the output Verilog parses cleanly through
// nl_compiler::from_vast -> Cell::from_id -> CellType::from_str.
//
// Port convention (CONFIRMED against live pipeline errors, not docs --
// CellType's real ports are NOT uniform across gate families):
//   AND2 / NAND2 / OR2 / NOR2 : inputs A1, A2  |  output ZN
//   XOR2                      : inputs A, B    |  output Z
//   XNOR2                     : inputs A, B    |  output ZN
//   NOT / BUF                 : input A        |  output Y  (UNCONFIRMED --
//                                no positive test yet; fix if pipeline errors)
//   MUX2                      : UNCONFIRMED -- verify against pipeline before
//                                relying on this in a benchmark
//
// Note: Yosys only generates the 2-input primitive cells ($_NAND_, $_AND_,
// etc.) from gate-level Verilog -- higher-fanin gates are decomposed. We
// therefore only need the 2-input (and 1-input) variants here. If you feed
// Yosys behavioural RTL and run synth, you may see other cell types; extend
// this file as needed, and confirm new port names against a real pipeline
// error before trusting them.

(* techmap_celltype = "$_NAND_" *)
module _NAND_ (A, B, Y);
  input A, B;
  output Y;
  NAND2 _TECHMAP_REPLACE_ (.A1(A), .A2(B), .ZN(Y));
endmodule

(* techmap_celltype = "$_AND_" *)
module _AND_ (A, B, Y);
  input A, B;
  output Y;
  AND2 _TECHMAP_REPLACE_ (.A1(A), .A2(B), .ZN(Y));
endmodule

(* techmap_celltype = "$_NOR_" *)
module _NOR_ (A, B, Y);
  input A, B;
  output Y;
  NOR2 _TECHMAP_REPLACE_ (.A1(A), .A2(B), .ZN(Y));
endmodule

(* techmap_celltype = "$_OR_" *)
module _OR_ (A, B, Y);
  input A, B;
  output Y;
  OR2 _TECHMAP_REPLACE_ (.A1(A), .A2(B), .ZN(Y));
endmodule

(* techmap_celltype = "$_XOR_" *)
module _XOR_ (A, B, Y);
  input A, B;
  output Y;
  XOR2 _TECHMAP_REPLACE_ (.A(A), .B(B), .Z(Y));
endmodule

(* techmap_celltype = "$_XNOR_" *)
module _XNOR_ (A, B, Y);
  input A, B;
  output Y;
  XNOR2 _TECHMAP_REPLACE_ (.A(A), .B(B), .ZN(Y));
endmodule

// UNCONFIRMED port convention below -- verify against a real pipeline error
// before trusting these in a benchmark run.

(* techmap_celltype = "$_NOT_" *)
module _NOT_ (A, Y);
  input A;
  output Y;
  NOT _TECHMAP_REPLACE_ (.A(A), .Y(Y));
endmodule

(* techmap_celltype = "$_BUF_" *)
module _BUF_ (A, Y);
  input A;
  output Y;
  BUF _TECHMAP_REPLACE_ (.A(A), .Y(Y));
endmodule

// MUX -- Yosys may generate these from behavioural RTL. Port names here are
// a guess by analogy and are NOT confirmed.
(* techmap_celltype = "$_MUX_" *)
module _MUX_ (A, B, S, Y);
  input A, B, S;
  output Y;
  MUX2 _TECHMAP_REPLACE_ (.A(A), .B(B), .S(S), .Y(Y));
endmodule
module c17 (N1, N2, N3, N6, N7, N22, N23);
  input N1;
  wire N1;
  input N2;
  wire N2;
  input N3;
  wire N3;
  input N6;
  wire N6;
  input N7;
  wire N7;
  output N22;
  wire N22;
  output N23;
  wire N23;
  wire N10;
  wire N11;
  wire N16;
  wire N19;

  NAND2 g0 (.A1(N1),  .A2(N3),  .ZN(N10));
  NAND2 g1 (.A1(N3),  .A2(N6),  .ZN(N11));
  NAND2 g2 (.A1(N2),  .A2(N11), .ZN(N16));
  NAND2 g3 (.A1(N11), .A2(N7),  .ZN(N19));
  NAND2 g4 (.A1(N10), .A2(N16), .ZN(N22));
  NAND2 g5 (.A1(N16), .A2(N19), .ZN(N23));
endmodule
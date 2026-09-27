module bench1(N1, N2, N3, N4, N5);
  input N1;
  input N2;
  input N3;
  input N4;
  output N5;
  wire N10;
  wire N11;

  NAND2 g0 (.A1(N1), .A2(N2), .ZN(N10));
  NOR2 g1 (.A1(N3), .A2(N4), .ZN(N11));
  XOR2 g2 (.A(N10), .B(N11), .Z(N5));
endmodule
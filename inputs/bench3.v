module bench3(N1, N2, N3, N4, N5, N6, N7, N8, N30);
  input N1;
  input N2;
  input N3;
  input N4;
  input N5;
  input N6;
  input N7;
  input N8;
  output N30;
  wire N10;
  wire N11;
  wire N12;
  wire N13;
  wire N20;
  wire N21;

  AND2 g0 (.A1(N1), .A2(N2), .ZN(N10));
  AND2 g1 (.A1(N3), .A2(N4), .ZN(N11));
  OR2 g2 (.A1(N5), .A2(N6), .ZN(N12));
  OR2 g3 (.A1(N7), .A2(N8), .ZN(N13));
  NAND2 g4 (.A1(N10), .A2(N11), .ZN(N20));
  NOR2 g5 (.A1(N12), .A2(N13), .ZN(N21));
  XOR2 g6 (.A(N20), .B(N21), .Z(N30));
endmodule
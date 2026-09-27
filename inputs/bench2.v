module bench2(N1, N2, N3, N4, N5, N20, N21);
  input N1;
  input N2;
  input N3;
  input N4;
  input N5;
  output N20;
  output N21;
  wire N10;
  wire N11;
  wire N12;
  wire N13;

  AND2 g0 (.A1(N1), .A2(N2), .ZN(N10));
  OR2 g1 (.A1(N3), .A2(N4), .ZN(N11));
  NAND2 g2 (.A1(N10), .A2(N11), .ZN(N12));
  XNOR2 g3 (.A(N11), .B(N5), .ZN(N13));
  NOR2 g4 (.A1(N12), .A2(N13), .ZN(N20));
  XOR2 g5 (.A(N10), .B(N13), .Z(N21));
endmodule
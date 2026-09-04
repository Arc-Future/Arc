//! L1 批量：值类型与结构体回归集。
//!
//! 所有 case 合并为单次 assert_compiles_batch。

use arc_tests::assert_compiles_batch;

#[test]
fn compiles_value_types_batch() {
    assert_compiles_batch(
        "value_types",
        &[
            (
                "struct_min",
                r#"using Arc;
struct VtPoint {
    public int X;
    public int Y;
    public VtPoint(int x, int y) { X = x; Y = y; }
    public int Sum() { return X + Y; }
}
void Main() {
    VtPoint p = new VtPoint(3, 4);
    int s = p.Sum();
    Console.WriteLine("struct_min_ok");
}
"#,
            ),
            (
                "struct_boxing",
                r#"using Arc;
struct VtPoint2 {
    public int X;
    public int Y;
    public VtPoint2(int x, int y) { X = x; Y = y; }
}
void Main() {
    VtPoint2 p = new VtPoint2(3, 4);
    object o = p;
    VtPoint2 q = (VtPoint2)o;
    bool isP = o is VtPoint2;
    Console.WriteLine("struct_boxing_ok");
}
"#,
            ),
            (
                "struct_static",
                r#"using Arc;
struct VtVector3 {
    public double X;
    public double Y;
    public double Z;
    public VtVector3(double x, double y, double z) { X = x; Y = y; Z = z; }
    public static readonly VtVector3 Zero = new VtVector3(0.0, 0.0, 0.0);
}
void Main() {
    Console.WriteLine("struct_static_ok");
}
"#,
            ),
            (
                "struct_selfref",
                r#"using Arc;
struct VtNode {
    public double Val;
    public VtNode(double v) { Val = v; }
    public static readonly VtNode Default = new VtNode(1.0);
}
void Main() {
    VtNode n = VtNode.Default;
    Console.WriteLine("struct_selfref_ok");
}
"#,
            ),
            (
                "struct_out_ref",
                r#"using Arc;
struct VtPair {
    public int A;
    public int B;
    public VtPair(int a, int b) { A = a; B = b; }
    public void Deconstruct(out int x, out int y) { x = A; y = B; }
}
void Main() {
    VtPair p = new VtPair(7, 8);
    int x;
    int y;
    p.Deconstruct(out x, out y);
    Console.WriteLine("struct_out_ref_ok");
}
"#,
            ),
            (
                "struct_multi_field",
                r#"using Arc;
struct VtRect {
    public double Width;
    public double Height;
    public VtRect(double w, double h) { Width = w; Height = h; }
    public double GetArea() { return Width * Height; }
    public double GetPerim() { return 2.0 * (Width + Height); }
}
void Main() {
    VtRect r = new VtRect(3.0, 4.0);
    double a = r.GetArea();
    double p = r.GetPerim();
    Console.WriteLine("struct_multi_ok");
}
"#,
            ),
            (
                "struct_nested",
                r#"using Arc;
struct VtInner {
    public int Val;
    public VtInner(int v) { Val = v; }
}
struct VtOuter {
    public VtInner Inner;
    public VtOuter(VtInner i) { Inner = i; }
}
void Main() {
    VtOuter o = new VtOuter(new VtInner(42));
    Console.WriteLine("struct_nested_ok");
}
"#,
            ),
            (
                "struct_array",
                r#"using Arc;
struct VtCell {
    public int V;
    public VtCell(int v) { V = v; }
}
void Main() {
    VtCell[] cells = new VtCell[3];
    cells[0] = new VtCell(1);
    cells[1] = new VtCell(2);
    cells[2] = new VtCell(3);
    Console.WriteLine("struct_array_ok");
}
"#,
            ),
            (
                "record_pos",
                r#"using Arc;
record VtRecPoint(int X, int Y);
void Main() {
    VtRecPoint p = new VtRecPoint(3, 4);
    Console.WriteLine(p.X.ToString());
}
"#,
            ),
            (
                "record_body",
                r#"using Arc;
record VtRecPerson {
    public string Name { get; set; }
    public int Age { get; set; }
}
void Main() {
    VtRecPerson p = new VtRecPerson();
    p.Name = "Ada";
    p.Age = 36;
    Console.WriteLine("record_body_ok");
}
"#,
            ),
        ],
    );
}

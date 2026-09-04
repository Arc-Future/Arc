using Arc;

enum Color {
    Red,
    Green,
    Blue
}

struct Pair {
    public int Left;
    public int Right;

    public int Sum() {
        return this.Left + this.Right;
    }
}

class Point {
    public int x;
    public int y = 0;
    private string label;

    public int Len() {
        return this.x + this.y;
    }
}

void Main() {
    Point p = new Point();
    p.x = 1;
    int[] xs = null;
    Color c = Color.Red;
    Pair pair = new Pair();
    pair.Left = 2;
    pair.Right = 3;
    int s = pair.Sum();
    Console.WriteLine(s);
}

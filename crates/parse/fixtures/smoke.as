using Arc;

namespace Demo {
    int Add(int x, int y) {
        int z = x + y;
        var w = z;
        return z;
    }
}

void Main() {
    int a = 1;
    bool ok = true;
    string s = "hi\n";
    char c = 'x';
    float f = 1.5;
    int n = Add(a, 2);
    char ch = s[0];
    Console.WriteLine(s);
    return;
}

using Arc;
using Arc.Collections;
using Arc.IO;

enum TokenKind {
    Identifier,
    Number,
    Eof
}

enum LexResult {
    Ok,
    Missing
}

class LexSpan {
    public int Start;
    public int Length;

    public string Label() {
        return "span";
    }
}

LexResult CheckInput(string content) {
    if (content.Length == 0) {
        return LexResult.Missing;
    }
    return LexResult.Ok;
}

void Main() {
    TokenKind kind = TokenKind.Number;
    switch (kind) {
        case Identifier:
        {
            Console.WriteLine("id");
            break;
        }
        case Number:
        {
            Console.WriteLine("num");
            break;
        }
        case Eof:
        {
            Console.WriteLine("eof");
            break;
        }
    }

    string diagnostic = "lex:" + "number";
    Console.WriteLine(diagnostic);

    List<int> nums = new List<int>();
    nums.Add(10);
    nums.Add(20);
    nums.Add(30);
    if (nums.Count == 3 && nums[0] == 10 && nums[2] == 30) {
        Console.WriteLine("list ok");
    }

    Dictionary<string, int> counts = new Dictionary<string, int>();
    counts["alpha"] = 1;
    counts["beta"] = 2;
    if (counts["alpha"] == 1 && counts["beta"] == 2) {
        Console.WriteLine("dict ok");
    }

    string content = File.ReadAllText("input.txt");

    LexSpan s = new LexSpan();
    s.Start = 0;
    s.Length = content.Length;
    Console.WriteLine(s.Label());

    LexResult result = CheckInput(content);
    switch (result) {
        case Ok:
        {
            Console.WriteLine("ok:3");
            break;
        }
        case Missing:
        {
            Console.WriteLine("err:missing");
            break;
        }
    }

    Console.WriteLine("smoke:ok");
}

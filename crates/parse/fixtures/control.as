using Arc;

enum Mode {
    A,
    B
}

void Main() {
    int i = 0;
    bool flag = !false;
    if (flag) {
        i = 1;
    } else {
        i = 2;
    }
    while (i < 10) {
        if (i == 5) {
            break;
        }
        i = i + 1;
        continue;
    }
    for (int j = 0; j < 3; j = j + 1) {
        i = i + j;
    }
    Mode m = Mode.A;
    switch (m) {
        case A:
            i = 100;
            break;
        case B:
            i = 200;
            break;
        default:
            i = 0;
            break;
    }
    Console.WriteLine(i);
}

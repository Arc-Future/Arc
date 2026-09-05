namespace UnitTest.Core;

// internal 跨文件测试辅助类型：由 AccessOopTests.as（同一编译单元）使用。
internal class InternalVault {
    public int Code;

    public InternalVault(int code) {
        Code = code;
    }

    public int Open() {
        return Code;
    }
}

internal class InternalOps {
    public static int Combine(InternalVault a, InternalVault b) {
        return a.Code * 100 + b.Code;
    }
}

// 顶层无修饰符类：默认可见性（RFC 025：顶层默认 internal，同编译单元可见）。
class DefaultLevel {
    public int Kick() {
        return 5;
    }
}

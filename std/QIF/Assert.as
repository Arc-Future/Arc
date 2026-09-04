namespace Arc.QIF;

using Arc;

/// <summary>
/// XUnit 风格静态断言类。对标 C# XUnit.Assert。
///
/// 失败行为：抛 <see cref="Arc.Exception"/>，由 Phase 2c 合成 host 的 try/catch
/// 记为 Fail；<see cref="Skip"/> 抛 <c>QIF_SKIP:</c> 前缀记为 Skipped。
/// （RFC 032 远期 <c>rt_qif_assert_*</c> facade / 动态 host **未**落地——勿冒充。）
///
/// 稳定面清单见 [RFC 032 QIF](../../../docs/rfc/032-qif.md)。
/// </summary>
public static class Assert {
    // ── 相等性断言 ──

    public static void Equal(int expected, int actual) {
        if (expected != actual) {
            throw new Arc.Exception("Assert.Equal failed. Expected: " + expected.ToString() + ", Actual: " + actual.ToString());
        }
    }

    public static void Equal(long expected, long actual) {
        if (expected != actual) {
            throw new Arc.Exception("Assert.Equal failed. Expected: " + expected.ToString() + ", Actual: " + actual.ToString());
        }
    }

    public static void Equal(string expected, string actual) {
        if (expected != actual) {
            throw new Arc.Exception("Assert.Equal failed. Expected: '" + expected + "', Actual: '" + actual + "'");
        }
    }

    public static void Equal(double expected, double actual, double delta) {
        double diff = expected - actual;
        if (diff < 0) { diff = -diff; }
        if (diff > delta) {
            throw new Arc.Exception("Assert.Equal failed. Expected: " + expected.ToString() + ", Actual: " + actual.ToString() + ", Delta: " + delta.ToString());
        }
    }

    public static void NotEqual(int notExpected, int actual) {
        if (notExpected == actual) {
            throw new Arc.Exception("Assert.NotEqual failed. Values are equal: " + actual.ToString());
        }
    }

    public static void NotEqual(long notExpected, long actual) {
        if (notExpected == actual) {
            throw new Arc.Exception("Assert.NotEqual failed. Values are equal: " + actual.ToString());
        }
    }

    public static void NotEqual(string notExpected, string actual) {
        if (notExpected == actual) {
            throw new Arc.Exception("Assert.NotEqual failed. Values are equal: '" + actual + "'");
        }
    }

    // ── 布尔断言 ──

    public static void True(bool condition) {
        if (!condition) {
            throw new Arc.Exception("Assert.True failed. Condition is false.");
        }
    }

    public static void True(bool condition, string message) {
        if (!condition) {
            throw new Arc.Exception(message);
        }
    }

    public static void False(bool condition) {
        if (condition) {
            throw new Arc.Exception("Assert.False failed. Condition is true.");
        }
    }

    public static void False(bool condition, string message) {
        if (condition) {
            throw new Arc.Exception(message);
        }
    }

    // ── null 检查 ──

    public static void Null(object value) {
        if (value != null) {
            throw new Arc.Exception("Assert.Null failed. Value is not null.");
        }
    }

    public static void NotNull(object value) {
        if (value == null) {
            throw new Arc.Exception("Assert.NotNull failed. Value is null.");
        }
    }

    public static void NotNull(object value, string message) {
        if (value == null) {
            throw new Arc.Exception(message);
        }
    }

    // ── 失败 ──

    public static void Fail(string message) {
        throw new Arc.Exception(message);
    }

    // ── 范围断言 ──

    /// <summary>断言 value 在 [low, high] 闭区间内。</summary>
    public static void InRange(int value, int low, int high) {
        if (value < low || value > high) {
            throw new Arc.Exception("Assert.InRange failed. Value " + value.ToString() + " not in [" + low.ToString() + ", " + high.ToString() + "]");
        }
    }

    /// <summary>断言 value 不在 [low, high] 闭区间内。</summary>
    public static void NotInRange(int value, int low, int high) {
        if (value >= low && value <= high) {
            throw new Arc.Exception("Assert.NotInRange failed. Value " + value.ToString() + " is in [" + low.ToString() + ", " + high.ToString() + "]");
        }
    }

    // ── 集合断言 ──

    /// <summary>断言集合非空。type arg 可由实参推断（如 <c>Assert.NotEmpty(xs)</c>）。</summary>
    public static void NotEmpty<T>(List<T> collection) {
        if (collection == null) {
            throw new Arc.Exception("Assert.NotEmpty failed. Collection is null.");
        }
        if (collection.Count == 0) {
            throw new Arc.Exception("Assert.NotEmpty failed. Collection is empty.");
        }
    }

    /// <summary>断言集合为空。type arg 可由实参推断（如 <c>Assert.Empty(xs)</c>）。</summary>
    public static void Empty<T>(List<T> collection) {
        if (collection == null) {
            throw new Arc.Exception("Assert.Empty failed. Collection is null.");
        }
        if (collection.Count != 0) {
            throw new Arc.Exception("Assert.Empty failed. Collection has " + collection.Count.ToString() + " elements.");
        }
    }

    /// <summary>断言集合包含指定元素。</summary>
    public static void Contains<T>(T expected, List<T> collection) {
        if (collection == null) {
            throw new Arc.Exception("Assert.Contains failed. Collection is null.");
        }
        int i = 0;
        while (i < collection.Count) {
            if (collection[i] == expected) {
                return;
            }
            i = i + 1;
        }
        throw new Arc.Exception("Assert.Contains failed. Element not found in collection.");
    }

    /// <summary>断言字符串包含子串。</summary>
    public static void Contains(string expectedSubstring, string actualString) {
        if (expectedSubstring == null || actualString == null) {
            throw new Arc.Exception("Assert.Contains failed. Arguments cannot be null.");
        }
        if (expectedSubstring == "") {
            return; // 空串视为包含于任意字符串
        }
        int expectedLen = expectedSubstring.Length;
        int actualLen = actualString.Length;
        if (expectedLen > actualLen) {
            throw new Arc.Exception("Assert.Contains failed. '" + expectedSubstring + "' not found in '" + actualString + "'");
        }
        int maxStart = actualLen - expectedLen;
        int start = 0;
        while (start <= maxStart) {
            string slice = actualString.Substring(start, expectedLen);
            if (slice == expectedSubstring) {
                return;
            }
            start = start + 1;
        }
        throw new Arc.Exception("Assert.Contains failed. '" + expectedSubstring + "' not found in '" + actualString + "'");
    }

    /// <summary>断言字符串以指定前缀开头（L1 断言面加深；非 List 元素路径）。</summary>
    public static void StartsWith(string expectedPrefix, string actualString) {
        if (expectedPrefix == null || actualString == null) {
            throw new Arc.Exception("Assert.StartsWith failed. Arguments cannot be null.");
        }
        if (!actualString.StartsWith(expectedPrefix)) {
            throw new Arc.Exception("Assert.StartsWith failed. Expected prefix '" + expectedPrefix + "', actual '" + actualString + "'");
        }
    }

    /// <summary>断言字符串以指定后缀结尾（L1 断言面加深；非 List 元素路径）。</summary>
    public static void EndsWith(string expectedSuffix, string actualString) {
        if (expectedSuffix == null || actualString == null) {
            throw new Arc.Exception("Assert.EndsWith failed. Arguments cannot be null.");
        }
        if (!actualString.EndsWith(expectedSuffix)) {
            throw new Arc.Exception("Assert.EndsWith failed. Expected suffix '" + expectedSuffix + "', actual '" + actualString + "'");
        }
    }

    /// <summary>断言集合不包含指定元素。</summary>
    public static void DoesNotContain<T>(T unexpected, List<T> collection) {
        if (collection == null) {
            throw new Arc.Exception("Assert.DoesNotContain failed. Collection is null.");
        }
        int i = 0;
        while (i < collection.Count) {
            if (collection[i] == unexpected) {
                throw new Arc.Exception("Assert.DoesNotContain failed. Unexpected element found in collection.");
            }
            i = i + 1;
        }
    }

    /// <summary>断言字符串不包含指定子串（与 <see cref="Contains(string, string)"/> 对称；L1 断言面加深）。</summary>
    public static void DoesNotContain(string unexpectedSubstring, string actualString) {
        if (unexpectedSubstring == null || actualString == null) {
            throw new Arc.Exception("Assert.DoesNotContain failed. Arguments cannot be null.");
        }
        if (unexpectedSubstring == "") {
            throw new Arc.Exception("Assert.DoesNotContain failed. Empty substring is contained in every string.");
        }
        int unexpectedLen = unexpectedSubstring.Length;
        int actualLen = actualString.Length;
        if (unexpectedLen > actualLen) {
            return;
        }
        int maxStart = actualLen - unexpectedLen;
        int start = 0;
        while (start <= maxStart) {
            string slice = actualString.Substring(start, unexpectedLen);
            if (slice == unexpectedSubstring) {
                throw new Arc.Exception("Assert.DoesNotContain failed. Unexpected substring '" + unexpectedSubstring + "' found in '" + actualString + "'");
            }
            start = start + 1;
        }
    }

    // ── 单元素断言 ──

    /// <summary>断言集合恰好包含一个元素。type arg 可由实参推断（如 <c>Assert.Single(xs)</c>）。</summary>
    public static void Single<T>(List<T> collection) {
        if (collection == null) {
            throw new Arc.Exception("Assert.Single failed. Collection is null.");
        }
        if (collection.Count != 1) {
            throw new Arc.Exception("Assert.Single failed. Expected 1 element, found " + collection.Count.ToString());
        }
    }

    // ── 谓词断言 ──

    /// <summary>断言集合中所有元素都满足 predicate。</summary>
    public static void All<T>(List<T> collection, Func<T, bool> predicate) {
        if (collection == null) {
            throw new Arc.Exception("Assert.All failed. Collection is null.");
        }
        int i = 0;
        while (i < collection.Count) {
            if (!predicate(collection[i])) {
                throw new Arc.Exception("Assert.All failed. Element at index " + i.ToString() + " does not satisfy the predicate.");
            }
            i = i + 1;
        }
    }

    /// <summary>断言集合中至少有一个元素满足 predicate。</summary>
    public static void Any<T>(List<T> collection, Func<T, bool> predicate) {
        if (collection == null) {
            throw new Arc.Exception("Assert.Any failed. Collection is null.");
        }
        int i = 0;
        while (i < collection.Count) {
            if (predicate(collection[i])) {
                return;
            }
            i = i + 1;
        }
        throw new Arc.Exception("Assert.Any failed. No element satisfies the predicate.");
    }

    /// <summary>断言两个集合元素按顺序相等。</summary>
    public static void SequenceEqual<T>(List<T> expected, List<T> actual) {
        if (expected == null || actual == null) {
            throw new Arc.Exception("Assert.SequenceEqual failed. One of the collections is null.");
        }
        if (expected.Count != actual.Count) {
            throw new Arc.Exception("Assert.SequenceEqual failed. Count differs: expected " + expected.Count.ToString() + ", actual " + actual.Count.ToString());
        }
        int i = 0;
        while (i < expected.Count) {
            if (expected[i] != actual[i]) {
                throw new Arc.Exception("Assert.SequenceEqual failed. Element at index " + i.ToString() + " differs.");
            }
            i = i + 1;
        }
    }

    // ── 比较断言 ──

    public static void Greater(int value, int threshold) {
        if (value <= threshold) {
            throw new Arc.Exception("Assert.Greater failed. " + value.ToString() + " <= " + threshold.ToString());
        }
    }

    public static void GreaterOrEqual(int value, int threshold) {
        if (value < threshold) {
            throw new Arc.Exception("Assert.GreaterOrEqual failed. " + value.ToString() + " < " + threshold.ToString());
        }
    }

    public static void Less(int value, int threshold) {
        if (value >= threshold) {
            throw new Arc.Exception("Assert.Less failed. " + value.ToString() + " >= " + threshold.ToString());
        }
    }

    public static void LessOrEqual(int value, int threshold) {
        if (value > threshold) {
            throw new Arc.Exception("Assert.LessOrEqual failed. " + value.ToString() + " > " + threshold.ToString());
        }
    }

    // ── 异常断言 ──

    /// <summary>
    /// 断言 action 抛出异常（不限定类型）。用于 Phase 2c 兼容。
    /// 推荐使用带 errorCode 参数的重载以获得精确类型匹配。
    /// </summary>
    public static void Throws(string actionName, Action action) {
        try {
            action.Invoke();
        } catch (Exception ex) {
            return; // 任何异常都视为通过
        }
        throw new Arc.Exception("Assert.Throws failed. No exception was thrown by '" + actionName + "'");
    }

    /// <summary>
    /// 断言 action 抛出指定错误码的异常。
    /// <param name="errorCode">错误码前缀（如 "E0340"），空字符串匹配任何异常。</param>
    /// <param name="actionName">动作描述（用于失败消息）。</param>
    /// <param name="action">要执行的动作。</param>
    /// </summary>
    public static void Throws(string errorCode, string actionName, Action action) {
        try {
            action.Invoke();
        } catch (Exception ex) {
            if (errorCode == "" || ex.Message.StartsWith(errorCode)) {
                return;
            }
            throw new Arc.Exception("Assert.Throws failed. Expected error '" + errorCode + "', got: " + ex.Message);
        }
        throw new Arc.Exception("Assert.Throws failed. No exception was thrown by '" + actionName + "'. Expected: " + errorCode);
    }

    // ── 泛型版 Throws / IsType / Single(predicate) 延后实现——
    // Arc MIR lowering 对泛型 + 委托 + is T 组合支持仍在完善中。
    // 运行时行为由非泛型重载（Throws(string, Action) / Throws(code, name, Action)）覆盖。
    // public static void Throws<T>(string actionName, Action action) { ... }
    // public static void IsType<T>(object value) { ... }
    // public static void Single<T>(List<T> collection, Func<T, bool> predicate) { ... }

    /// <summary>
    /// 断言 action 不抛出任何异常。
    /// <param name="actionName">动作描述（用于失败消息）。</param>
    /// <param name="action">要执行的动作。</param>
    /// </summary>
    public static void DoesNotThrow(string actionName, Action action) {
        try {
            action.Invoke();
        } catch (Exception ex) {
            throw new Arc.Exception("Assert.DoesNotThrow failed. Unexpected exception from '" + actionName + "': " + ex.Message);
        }
    }

    // ── 类型断言（泛型版延后实现） ──
    // public static void IsType<T>(object value) { ... }

    // ── 单元素谓词断言（泛型版延后实现） ──
    // public static void Single<T>(List<T> collection, Func<T, bool> predicate) { ... }

    // ── 跳过（测试框架内部使用） ──

    /// <summary>标记测试为跳过。</summary>
    public static void Skip(string reason) {
        throw new Arc.Exception("QIF_SKIP: " + reason);
    }
}

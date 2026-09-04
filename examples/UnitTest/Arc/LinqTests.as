namespace UnitTest.Arc;

using Arc;
using Arc.Collections;
using Arc.Linq;
using Arc.QIF;

/// <summary>
/// LINQ to Objects 诚实子集：query / 方法链 <c>Where</c>/<c>Select</c>/<c>OrderBy</c>
/// （OrderBy 真排序：缓冲 List + Sort comparator；key 捕获或无支持比较时诚实跳过）；
/// 终端 <c>Any</c>/<c>Count</c>/<c>First</c>/<c>FirstOrDefault</c>（数组 + List；MIR 编译期展开）。
/// Queryable 未落地——禁止冒充完备。
/// </summary>
public class LinqTests
{
    [Fact]
    public void Linq_Where_Filter()
    {
        int[] nums = [5, 15, 8, 20, 3];
        int count = 0;
        foreach (var x in (from n in nums where n > 10 select n)) {
            count = count + 1;
        }
        Assert.Equal(2, count);
    }

    [Fact]
    public void Linq_Select_Transform()
    {
        int[] nums = [1, 2, 3];
        int sum = 0;
        foreach (var x in (from n in nums select n * 2)) {
            sum = sum + x;
        }
        Assert.Equal(12, sum);
    }

    [Fact]
    public void Linq_WhereSelect_Chained()
    {
        int[] nums = [5, 15, 8, 20, 3];
        int sum = 0;
        foreach (var x in (from n in nums where n > 10 select n * 2)) {
            sum = sum + x;
        }
        Assert.Equal(70, sum);
    }

    [Fact]
    public void Linq_Where_NoMatch()
    {
        int[] nums = [1, 2, 3];
        int count = 0;
        foreach (var x in (from n in nums where n > 100 select n)) {
            count = count + 1;
        }
        Assert.Equal(0, count);
    }

    [Fact]
    public void Linq_ListSource_WhereSelect()
    {
        List<int> nums = new List<int>();
        nums.Add(5);
        nums.Add(15);
        nums.Add(8);
        nums.Add(20);
        int sum = 0;
        foreach (var x in (from n in nums where n > 10 select n * 2)) {
            sum = sum + x;
        }
        Assert.Equal(70, sum);
    }

    [Fact]
    public void Linq_Materialize_CountViaForeach()
    {
        // 赋值物化 `List<int> xs = from …` 的 typeck 目标类型仍后置；
        // 此处用 foreach 计数证明 where 过滤诚实可用（非 Fact-Skip）。
        int[] nums = [5, 15, 8, 20, 3];
        int count = 0;
        int first = 0;
        int second = 0;
        foreach (var x in (from n in nums where n > 10 select n)) {
            if (count == 0) { first = x; }
            if (count == 1) { second = x; }
            count = count + 1;
        }
        Assert.Equal(2, count);
        Assert.Equal(15, first);
        Assert.Equal(20, second);
    }

    [Fact]
    public void Linq_MethodChain_WhereSelect()
    {
        // 方法链与 query 在 Where/Select 上语义对齐（MIR try_lower_linq_chain）。
        int[] nums = [5, 15, 8, 20, 3];
        int sum = 0;
        foreach (var x in nums.Where(n => n > 10).Select(n => n * 2)) {
            sum = sum + x;
        }
        Assert.Equal(70, sum);
    }

    [Fact]
    public void Linq_List_MethodChain_WhereSelect()
    {
        List<int> nums = new List<int>();
        nums.Add(1);
        nums.Add(4);
        nums.Add(9);
        nums.Add(16);
        int count = 0;
        foreach (var x in nums.Where(n => n >= 9).Select(n => n)) {
            count = count + 1;
        }
        Assert.Equal(2, count);
    }

    [Fact]
    public void Linq_OrderBy_Sorts()
    {
        // 真排序：orderby n 将 5,8,3 升序为 3,5,8（缓冲 List + rt_list_sort）。
        int[] nums = [5, 8, 3];
        int i = 0;
        int a = 0;
        int b = 0;
        int c = 0;
        foreach (var x in (from n in nums orderby n select n)) {
            if (i == 0) { a = x; }
            if (i == 1) { b = x; }
            if (i == 2) { c = x; }
            i = i + 1;
        }
        Assert.Equal(3, i);
        Assert.Equal(3, a);
        Assert.Equal(5, b);
        Assert.Equal(8, c);
    }

    [Fact]
    public void Linq_OrderBy_Descending()
    {
        int[] nums = [5, 8, 3];
        int i = 0;
        int a = 0;
        int b = 0;
        int c = 0;
        foreach (var x in (from n in nums orderby n descending select n)) {
            if (i == 0) { a = x; }
            if (i == 1) { b = x; }
            if (i == 2) { c = x; }
            i = i + 1;
        }
        Assert.Equal(3, i);
        Assert.Equal(8, a);
        Assert.Equal(5, b);
        Assert.Equal(3, c);
    }

    [Fact]
    public void Linq_OrderBy_List_Source()
    {
        List<int> nums = new List<int>();
        nums.Add(8);
        nums.Add(3);
        nums.Add(5);
        int i = 0;
        int a = 0;
        int b = 0;
        int c = 0;
        foreach (var x in nums.OrderBy(n => n)) {
            if (i == 0) { a = x; }
            if (i == 1) { b = x; }
            if (i == 2) { c = x; }
            i = i + 1;
        }
        Assert.Equal(3, i);
        Assert.Equal(3, a);
        Assert.Equal(5, b);
        Assert.Equal(8, c);
    }

    [Fact]
    public void Linq_OrderBy_AfterWhere()
    {
        int[] nums = [5, 15, 8, 20, 3];
        int i = 0;
        int a = 0;
        int b = 0;
        foreach (var x in (from n in nums where n > 10 orderby n select n)) {
            if (i == 0) { a = x; }
            if (i == 1) { b = x; }
            i = i + 1;
        }
        Assert.Equal(2, i);
        Assert.Equal(15, a);
        Assert.Equal(20, b);
    }

    [Fact]
    public void Linq_OrderBy_FieldKey()
    {
        // 字段 key：orderby p.Age → 3,5,8（比较器对缓冲 List_Person 的 Age 三值化）。
        List<LinqPerson> people = new List<LinqPerson>();
        people.Add(new LinqPerson("zoe", 3));
        people.Add(new LinqPerson("bob", 5));
        people.Add(new LinqPerson("amy", 8));
        int i = 0;
        int a = 0;
        int b = 0;
        int c = 0;
        foreach (var p in (from p in people orderby p.Age select p)) {
            if (i == 0) { a = p.Age; }
            if (i == 1) { b = p.Age; }
            if (i == 2) { c = p.Age; }
            i = i + 1;
        }
        Assert.Equal(3, i);
        Assert.Equal(3, a);
        Assert.Equal(5, b);
        Assert.Equal(8, c);
    }

    [Fact]
    public void Linq_OrderBy_Terminal()
    {
        int[] nums = [5, 8, 3];
        Assert.Equal(3, nums.OrderBy(n => n).First());
        Assert.Equal(8, nums.OrderByDescending(n => n).First());
        Assert.Equal(3, nums.OrderBy(n => n).Count());
        int[] nums2 = [5, 15, 8, 20, 3];
        Assert.Equal(15, nums2.Where(n => n > 10).OrderBy(n => n).First());
    }

    [Fact]
    public void Linq_Any_Count_First_Array()
    {
        int[] nums = [5, 15, 8, 20, 3];
        Assert.True(nums.Any());
        Assert.True(nums.Any(n => n > 10));
        Assert.False(nums.Any(n => n > 100));
        Assert.Equal(5, nums.Count());
        Assert.Equal(2, nums.Count(n => n > 10));
        Assert.Equal(5, nums.First());
        Assert.Equal(15, nums.First(n => n > 10));
    }

    [Fact]
    public void Linq_Any_Count_First_List()
    {
        List<int> nums = new List<int>();
        nums.Add(1);
        nums.Add(4);
        nums.Add(9);
        Assert.True(nums.Any());
        Assert.True(nums.Any(n => n >= 9));
        Assert.False(nums.Any(n => n < 0));
        Assert.Equal(3, nums.Count());
        Assert.Equal(2, nums.Count(n => n >= 4));
        Assert.Equal(1, nums.First());
        Assert.Equal(9, nums.First(n => n > 4));
    }

    [Fact]
    public void Linq_Terminal_AfterWhereSelect()
    {
        int[] nums = [5, 15, 8, 20, 3];
        Assert.True(nums.Where(n => n > 10).Any());
        Assert.Equal(2, nums.Where(n => n > 10).Count());
        Assert.Equal(15, nums.Where(n => n > 10).First());
        Assert.Equal(30, nums.Where(n => n > 10).Select(n => n * 2).First());
        Assert.Equal(2, nums.Where(n => n > 10).Select(n => n * 2).Count());
    }

    [Fact]
    public void Linq_Any_Empty_IsFalse()
    {
        int[] empty = [];
        Assert.False(empty.Any());
        Assert.Equal(0, empty.Count());
    }

    [Fact]
    public void Linq_FirstOrDefault_Array()
    {
        int[] nums = [5, 15, 8, 20, 3];
        Assert.Equal(5, nums.FirstOrDefault());
        Assert.Equal(15, nums.FirstOrDefault(n => n > 10));
        Assert.Equal(0, nums.FirstOrDefault(n => n > 100));
        int[] empty = [];
        Assert.Equal(0, empty.FirstOrDefault());
    }

    [Fact]
    public void Linq_FirstOrDefault_List_And_Chain()
    {
        List<int> nums = new List<int>();
        nums.Add(1);
        nums.Add(4);
        nums.Add(9);
        Assert.Equal(1, nums.FirstOrDefault());
        Assert.Equal(9, nums.FirstOrDefault(n => n > 4));
        Assert.Equal(0, nums.FirstOrDefault(n => n < 0));
        // List [1,4,9] → Where>4 → 9 → Select*2 → 18（勿抄数组用例的 30）
        Assert.Equal(18, nums.Where(n => n > 4).Select(n => n * 2).FirstOrDefault());
        List<int> empty = new List<int>();
        Assert.Equal(0, empty.FirstOrDefault());
    }

    // ── join（inner join，MIR 物化；内层源为 List<T>）──

    [Fact]
    public void Linq_Join_InnerJoin()
    {
        // order→cust 等值匹配；select 引用外层变量（内层变量引用受 MIR 展开
        // 限制，见文档诚实说明——不冒充完整 C# join 面）
        List<LinqOrder> orders = new List<LinqOrder>();
        orders.Add(new LinqOrder(1, 100, 10));
        orders.Add(new LinqOrder(2, 100, 20));
        orders.Add(new LinqOrder(3, 200, 30));
        List<LinqCust> custs = new List<LinqCust>();
        custs.Add(new LinqCust(100, "alice"));
        custs.Add(new LinqCust(200, "bob"));
        int count = 0;
        int sum = 0;
        foreach (var x in (from o in orders
                           join c in custs on o.CustId == c.Id
                           select o.Amount)) {
            count = count + 1;
            sum = sum + x;
        }
        Assert.Equal(3, count);
        Assert.Equal(60, sum);
    }

    [Fact]
    public void Linq_Join_NoMatch_Excluded()
    {
        // 孤儿订单（CustId=999 无对应客户）不参与 inner join
        List<LinqOrder> orders = new List<LinqOrder>();
        orders.Add(new LinqOrder(1, 100, 10));
        orders.Add(new LinqOrder(2, 999, 20));
        List<LinqCust> custs = new List<LinqCust>();
        custs.Add(new LinqCust(100, "alice"));
        int count = 0;
        foreach (var x in (from o in orders
                           join c in custs on o.CustId == c.Id
                           select o.Amount)) {
            count = count + 1;
        }
        Assert.Equal(1, count);
    }

    // ── group … by … [into g]（MIR 物化；产物 Grouping<K,T>）──

    [Fact]
    public void Linq_GroupBy_Into()
    {
        List<LinqPerson> people = new List<LinqPerson>();
        people.Add(new LinqPerson("a1", 3));
        people.Add(new LinqPerson("a2", 3));
        people.Add(new LinqPerson("b1", 8));
        int groupCount = 0;
        int k3 = 0;
        int k8 = 0;
        foreach (var g in (from p in people group p by p.Age into g select g)) {
            if (g.Key == 3) { k3 = g.Count; }
            if (g.Key == 8) { k8 = g.Count; }
            groupCount = groupCount + 1;
        }
        Assert.Equal(2, groupCount);
        Assert.Equal(2, k3);
        Assert.Equal(1, k8);
    }

    // ── let（MIR 多变量流；中间变量参与后续 where/select）──

    [Fact]
    public void Linq_Let_ThenWhere()
    {
        int[] nums = [3, 6, 9];
        int sum = 0;
        foreach (var x in (from n in nums let d = n * 2 where d > 10 select d)) {
            sum = sum + x;
        }
        Assert.Equal(30, sum);
    }

    // ── 多键排序：orderby k1, k2 折叠为单 comparator（先 k1 后 k2）──

    [Fact]
    public void Linq_OrderBy_MultiKey()
    {
        List<LinqPerson> people = new List<LinqPerson>();
        people.Add(new LinqPerson("z", 3));
        people.Add(new LinqPerson("a", 3));
        people.Add(new LinqPerson("m", 8));
        // 先按 Age 升序；同 Age 内按 Name 升序 → 3/a, 3/z, 8/m
        int i = 0;
        string first = "";
        string second = "";
        string third = "";
        foreach (var p in (from p in people orderby p.Age, p.Name select p)) {
            if (i == 0) { first = p.Name; }
            if (i == 1) { second = p.Name; }
            if (i == 2) { third = p.Name; }
            i = i + 1;
        }
        Assert.Equal(3, i);
        Assert.True(first == "a");
        Assert.True(second == "z");
        Assert.True(third == "m");
    }

    // ── 泛型物化终端：ToList / ToArray（MIR lower_linq_terminal 整链物化）──

    [Fact]
    public void Linq_ToList_Materialize()
    {
        int[] nums = [5, 15, 8, 20, 3];
        List<int> list = nums.Where(n => n > 10).ToList();
        Assert.Equal(2, list.Count);
        Assert.Equal(15, list[0]);
        Assert.Equal(20, list[1]);
    }

    [Fact]
    public void Linq_ToArray_Materialize()
    {
        int[] nums = [1, 2, 3];
        int[] arr = nums.Select(n => n * 2).ToArray();
        Assert.Equal(3, arr.Length);
        Assert.Equal(2, arr[0]);
        Assert.Equal(4, arr[1]);
        Assert.Equal(6, arr[2]);
    }

    // ── join + 尾算子（where/select）组合：验证 join 路由到物化后尾算子续流 ──

    [Fact]
    public void Linq_Join_ThenWhere()
    {
        List<LinqOrder> orders = new List<LinqOrder>();
        orders.Add(new LinqOrder(1, 100, 10));
        orders.Add(new LinqOrder(2, 100, 20));
        orders.Add(new LinqOrder(3, 200, 30));
        List<LinqCust> custs = new List<LinqCust>();
        custs.Add(new LinqCust(100, "alice"));
        custs.Add(new LinqCust(200, "bob"));
        int count = 0;
        int sum = 0;
        foreach (var x in (from o in orders
                           join c in custs on o.CustId == c.Id
                           where o.Amount > 15
                           select o.Amount)) {
            count = count + 1;
            sum = sum + x;
        }
        Assert.Equal(2, count);
        Assert.Equal(50, sum);
    }

    // ── 多键排序混合方向：orderby k1, k2 descending（先 k1 升序，同键 k2 降序）──

    [Fact]
    public void Linq_OrderBy_MultiKey_MixedDir()
    {
        List<LinqPerson> people = new List<LinqPerson>();
        people.Add(new LinqPerson("z", 3));
        people.Add(new LinqPerson("a", 3));
        people.Add(new LinqPerson("m", 8));
        // 先按 Age 升序；同 Age 内按 Name 降序 → 3/z, 3/a, 8/m
        int i = 0;
        string first = "";
        string second = "";
        string third = "";
        foreach (var p in (from p in people orderby p.Age, p.Name descending select p)) {
            if (i == 0) { first = p.Name; }
            if (i == 1) { second = p.Name; }
            if (i == 2) { third = p.Name; }
            i = i + 1;
        }
        Assert.Equal(3, i);
        Assert.True(first == "z");
        Assert.True(second == "a");
        Assert.True(third == "m");
    }

    // ── let + select（无 where）：中间变量直接投影 ──

    [Fact]
    public void Linq_Let_ThenSelect()
    {
        int[] nums = [3, 6, 9];
        int sum = 0;
        foreach (var x in (from n in nums let d = n * 2 select d)) {
            sum = sum + x;
        }
        Assert.Equal(36, sum);
    }

    // ── 泛型物化终端：查询语法源 ──

    [Fact]
    public void Linq_ToList_QuerySyntax()
    {
        int[] nums = [5, 15, 8, 20, 3];
        List<int> list = (from n in nums where n > 10 select n).ToList();
        Assert.Equal(2, list.Count);
        Assert.Equal(15, list[0]);
        Assert.Equal(20, list[1]);
    }

    [Fact]
    public void Linq_ToArray_ListSource()
    {
        List<int> nums = new List<int>();
        nums.Add(1);
        nums.Add(4);
        nums.Add(9);
        int[] arr = nums.Select(n => n * 2).ToArray();
        Assert.Equal(3, arr.Length);
        Assert.Equal(2, arr[0]);
        Assert.Equal(8, arr[1]);
        Assert.Equal(18, arr[2]);
    }

    // ── 方法链字段 key + where（List<T> 源）──

    [Fact]
    public void Linq_MethodChain_FieldKey_Where()
    {
        List<LinqPerson> people = new List<LinqPerson>();
        people.Add(new LinqPerson("a", 3));
        people.Add(new LinqPerson("b", 5));
        people.Add(new LinqPerson("c", 8));
        int count = 0;
        int sum = 0;
        foreach (var p in people.Where(p => p.Age > 4)) {
            count = count + 1;
            sum = sum + p.Age;
        }
        Assert.Equal(2, count);
        Assert.Equal(13, sum);
    }
}

/// 字段 key 排序夹具（`orderby p.Age`）。
class LinqPerson
{
    public string Name;
    public int Age;
    public LinqPerson(string name, int age)
    {
        Name = name;
        Age = age;
    }
}

/// join 夹具：订单（外层）。
class LinqOrder
{
    public int Id;
    public int CustId;
    public int Amount;
    public LinqOrder(int id, int custId, int amount)
    {
        Id = id;
        CustId = custId;
        Amount = amount;
    }
}

/// join 夹具：客户（内层 List<T> 源）。
class LinqCust
{
    public int Id;
    public string Name;
    public LinqCust(int id, string name)
    {
        Id = id;
        Name = name;
    }
}

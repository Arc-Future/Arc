namespace UnitTest.Core;

using Arc;
using Arc.QIF;
using Arc.Collections;

interface IAnimal { int Id(); }
class Dog : IAnimal { public int Id() { return 42; } }
interface IGetter<out T> { T Get(); }
class DogBox : IGetter<Dog> { public Dog Get() { return new Dog(); } }

class DogEnumerator : IEnumerator<Dog> {
    private int _state;
    public DogEnumerator() { _state = 0; }
    public bool MoveNext() {
        if (_state == 0) { _state = 1; return true; }
        return false;
    }
    public Dog Current { get { return new Dog(); } }
}

class DogSeq : IEnumerable<Dog> {
    public IEnumerator<Dog> GetEnumerator() { return new DogEnumerator(); }
}

class AnimalComparer : IComparer<IAnimal> {
    public int Compare(IAnimal x, IAnimal y) { return x.Id() - y.Id(); }
}

public class VarianceTests
{
    [Fact]
    public void DirectCovariance_Assignment()
    {
        IGetter<Dog> d = new DogBox();
        IGetter<IAnimal> a = d;
        Assert.Equal(42, a.Get().Id());
    }

    [Fact]
    public void IEnumerable_Dog_Assigns_To_IEnumerable_IAnimal()
    {
        IEnumerable<Dog> dogs = new DogSeq();
        IEnumerable<IAnimal> animals = dogs;
        Assert.Equal(1, 1);
    }

    [Fact]
    public void IComparer_IAnimal_Assigns_To_IComparer_Dog()
    {
        IComparer<IAnimal> wide = new AnimalComparer();
        IComparer<Dog> narrow = wide;
        Assert.Equal(0, narrow.Compare(new Dog(), new Dog()));
    }

    // 数组元素 invariant（RFC 077 项 3）：同元素类型可赋；Dog[]→Animal[] 由
    // typeck / array_invariant_e2e 负向拒绝（不追 C# 数组协变）。
    [Fact]
    public void Array_SameElemType_Assign()
    {
        Dog[] a = [new Dog()];
        Dog[] b = a;
        Assert.Equal(42, b[0].Id());
    }
}

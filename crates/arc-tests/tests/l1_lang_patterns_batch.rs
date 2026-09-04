//! L1 批量：语言模式匹配回归集（6 case）。
//!
//! 从 lang_patterns_batch_e2e.rs 提取，改为 L1 纯编译期测试。

use arc_tests::assert_compiles_batch;

#[test]
fn compiles_lang_patterns_batch() {
    assert_compiles_batch(
        "lang_patterns",
        &[
            (
                "is_basic",
                r#"using Arc;

class Animal {
    public virtual string Kind() { return "animal"; }
}

class Dog : Animal {
    public override string Kind() { return "dog"; }
}

class Cat : Animal {
    public override string Kind() { return "cat"; }
}

void Main() {
    Animal a = new Dog();
    if (!(a is Dog)) { Console.WriteLine("fail:subclass"); return; }
    if (!(a is Animal)) { Console.WriteLine("fail:base"); return; }
    if (a is Cat) { Console.WriteLine("fail:sibling"); return; }
    Animal n = null;
    if (n is Dog) { Console.WriteLine("fail:null"); return; }
    if (a is int) { Console.WriteLine("fail:primitive"); return; }
    Console.WriteLine("is_basic_ok");
}
"#,
            ),
            (
                "is_constant",
                r#"using Arc;

void Main() {
    int n = 5;
    if (!(n is 5)) { Console.WriteLine("fail:int_true"); return; }
    if (n is 6) { Console.WriteLine("fail:int_false"); return; }

    string s = "hello";
    if (!(s is "hello")) { Console.WriteLine("fail:str_true"); return; }
    if (s is "world") { Console.WriteLine("fail:str_false"); return; }

    bool b = true;
    if (!(b is true)) { Console.WriteLine("fail:bool_true"); return; }
    if (b is false) { Console.WriteLine("fail:bool_false"); return; }

    char c = 'x';
    if (!(c is 'x')) { Console.WriteLine("fail:char_true"); return; }
    if (c is 'y') { Console.WriteLine("fail:char_false"); return; }

    string s2 = null;
    if (!(s2 is null)) { Console.WriteLine("fail:null_true"); return; }
    string t = "a";
    if (t is null) { Console.WriteLine("fail:null_false"); return; }

    int m = 2;
    if (!(m is 1 or 2)) { Console.WriteLine("fail:or_true"); return; }
    if (m is 3 or 4) { Console.WriteLine("fail:or_false"); return; }

    Console.WriteLine("is_constant_ok");
}
"#,
            ),
            (
                "is_pattern_m2",
                r#"using Arc;

class M2Animal { public virtual string Kind() { return "animal"; } }
class M2Dog : M2Animal { public override string Kind() { return "dog"; } }

void Main() {
    M2Animal a = new M2Dog();
    if (!(a is M2Dog)) { Console.WriteLine("fail:type_dog"); return; }
    if (!(a is M2Animal)) { Console.WriteLine("fail:type_animal"); return; }
    if (a is string) { Console.WriteLine("fail:type_string"); return; }

    if (a is M2Dog d) {
        if (!(d.Kind() == "dog")) { Console.WriteLine("fail:bind_kind"); return; }
    } else {
        Console.WriteLine("fail:bind_else");
        return;
    }

    string s = null;
    if (!(s is null)) { Console.WriteLine("fail:is_null"); return; }
    string t = "hello";
    if (t is null) { Console.WriteLine("fail:not_null"); return; }

    int x = 42;
    if (x is var y) {
        if (y != 42) { Console.WriteLine("fail:var_bind"); return; }
    }

    int n = 5;
    if (!(n is int)) { Console.WriteLine("fail:fold_int"); return; }
    if (n is double) { Console.WriteLine("fail:fold_double"); return; }
    bool b = true;
    if (!(b is bool)) { Console.WriteLine("fail:fold_bool"); return; }
    if (b is int) { Console.WriteLine("fail:fold_bool_int"); return; }

    Console.WriteLine("is_pattern_m2_ok");
}
"#,
            ),
            (
                "is_short_circuit",
                r#"using Arc;

class ScAnimal { public virtual string Kind() { return "animal"; } }
class ScDog : ScAnimal { public override string Kind() { return "dog"; } }

void Main() {
    object o1 = 5;
    int n1 = 5;
    if (!(n1 == 5 && o1 is int)) { Console.WriteLine("fail:and_true"); return; }
    if (n1 == 6 && o1 is int) { Console.WriteLine("fail:and_short"); return; }
    if (n1 == 5 && o1 is long) { Console.WriteLine("fail:and_false"); return; }

    if (!(n1 == 6 || o1 is int)) { Console.WriteLine("fail:or_eval"); return; }
    if (!(n1 == 5 || o1 is long)) { Console.WriteLine("fail:or_short"); return; }
    if (n1 == 6 || o1 is long) { Console.WriteLine("fail:or_false"); return; }

    if (!((n1 == 5 && o1 is int) || n1 == 6)) { Console.WriteLine("fail:nest1"); return; }
    if (!((n1 == 6 && o1 is long) || n1 == 5)) { Console.WriteLine("fail:nest2"); return; }
    if ((n1 == 6 && o1 is int) || (n1 == 7 && o1 is long)) { Console.WriteLine("fail:nest3"); return; }

    ScAnimal a = new ScDog();
    if (!(a != null && a is ScDog)) { Console.WriteLine("fail:ref_and_true"); return; }
    if (a != null && a is string) { Console.WriteLine("fail:ref_and_false"); return; }
    ScAnimal n = null;
    if (n != null && n is ScDog) { Console.WriteLine("fail:ref_null"); return; }

    Console.WriteLine("is_short_circuit_ok");
}
"#,
            ),
            (
                "pattern_combinator",
                r#"using Arc;

class CbAnimal { public virtual string Kind() { return "animal"; } }
class CbDog : CbAnimal { public override string Kind() { return "dog"; } }
class CbCat : CbAnimal { public override string Kind() { return "cat"; } }

void Main() {
    CbAnimal a = new CbDog();
    if (!(a is CbDog and CbAnimal)) { Console.WriteLine("fail:and_true"); return; }
    if (a is CbDog and CbCat) { Console.WriteLine("fail:and_false"); return; }
    if (!(a is CbDog or CbCat)) { Console.WriteLine("fail:or_true"); return; }
    if (!(a is CbCat or CbDog)) { Console.WriteLine("fail:or_true2"); return; }
    if (a is CbCat or string) { Console.WriteLine("fail:or_false"); return; }
    if (a is not CbDog) { Console.WriteLine("fail:not_false"); return; }
    if (!(a is not CbCat)) { Console.WriteLine("fail:not_true"); return; }
    if (!(a is (CbDog or CbCat) and CbAnimal)) { Console.WriteLine("fail:paren_true"); return; }
    if (a is (CbCat or string) and CbAnimal) { Console.WriteLine("fail:paren_false"); return; }
    if (a is not (CbDog or CbCat)) { Console.WriteLine("fail:not_paren"); return; }
    CbDog d1 = new CbDog();
    if (!(d1 is CbDog and not null)) { Console.WriteLine("fail:notnull_true"); return; }
    CbDog dn = null;
    if (dn is CbDog and not null) { Console.WriteLine("fail:notnull_false"); return; }
    if (a is CbDog d and not null) {
        if (!(d.Kind() == "dog")) { Console.WriteLine("fail:binding_kind"); return; }
    } else {
        Console.WriteLine("fail:binding_else");
        return;
    }
    Console.WriteLine("pattern_combinator_ok");
}
"#,
            ),
            (
                "type_pattern",
                r#"using Arc;

class TpDog {
    public virtual string Kind() { return "dog"; }
    public string Bark() { return "woof"; }
}

void Main() {
    object o = new TpDog();
    if (o is TpDog d) {
        if (!(d.Bark() == "woof")) { Console.WriteLine("fail:bark"); return; }
    } else {
        Console.WriteLine("fail:match");
        return;
    }
    Console.WriteLine("type_pattern_ok");
}
"#,
            ),
        ],
    );
}

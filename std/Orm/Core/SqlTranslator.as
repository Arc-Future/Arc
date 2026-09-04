// RFC 039 Sprint 2c: SQL 翻译器（Arc 语言实现）
//
// SqlTranslator 是表达式树 → SQL 的领域特定翻译器。所有 SQL 语法知识
// （WHERE/SELECT/ORDER BY 关键字、== → = 转换、字符串引号、列名提取等）
// 都集中在本文件中，表达式树（std/Linq/Expressions）保持领域无关。
//
// 翻译器通过 NodeType 分派 + 虚方法访问器遍历表达式树，无需 is/as 下转：
//   Call     → 按 Method 名分派（Table/Where/Select/OrderBy/OrderByDescending/ToList）
//   Lambda   → 递归翻译 Body
//   Binary   → 按 per-op NodeType 分派（Add/Subtract/Equal/AndAlso/…；RFC 039 Slice 2）
//   Unary    → 按 per-op NodeType 分派（Not/Negate）
//   Member   → Member（strip 参数前缀: u.Age → Age）
//   Parameter→ Name
//   Capture  → :Name（参数化占位符）
//   Constant → StringValue（int/bool 直接输出，string 加单引号）
namespace Arc.Orm;

using Arc.Linq.Expressions;

public class SqlTranslator {
    /// 翻译表达式树为完整 SQL 语句。
    public string Translate(Expression expression) {
        if (expression == null) {
            return "";
        }
        return this.TranslateNode(expression);
    }

    /// 按 NodeType 分派到具体的翻译方法。
    public string TranslateNode(Expression expr) {
        if (expr.NodeType == ExpressionType.Call) {
            return this.TranslateMethodCall(expr);
        }
        if (expr.NodeType == ExpressionType.Lambda) {
            Expression body = expr.GetBody();
            if (body != null) {
                return this.TranslateNode(body);
            }
            return "";
        }
        if (this.IsBinaryNode(expr)) {
            return this.TranslateBinary(expr);
        }
        if (this.IsUnaryNode(expr)) {
            return this.TranslateUnary(expr);
        }
        if (expr.NodeType == ExpressionType.Member) {
            // SQL 中只关心列名，忽略参数前缀: u.Age → Age
            return expr.GetMember();
        }
        if (expr.NodeType == ExpressionType.Parameter) {
            return expr.GetName();
        }
        if (expr.NodeType == ExpressionType.Capture) {
            // 参数化占位符
            return ":" + expr.GetName();
        }
        if (expr.NodeType == ExpressionType.Constant) {
            return this.TranslateConstant(expr);
        }
        if (expr.NodeType == ExpressionType.Conditional) {
            return this.TranslateConditional(expr);
        }
        if (expr.NodeType == ExpressionType.Cast) {
            return this.TranslateCast(expr);
        }
        if (expr.NodeType == ExpressionType.Index) {
            // SQLite 无原生数组/字典类型，索引访问无法直接映射为 SQL。
            // 显式返回空串标记 unsupported（非 silent miss）。
            return "";
        }
        if (expr.NodeType == ExpressionType.New) {
            // SQL 不支持对象构造（new T(...)）。显式返回空串标记 unsupported。
            return "";
        }
        return "";
    }

    /// RFC 039 Sprint 2d Slice 2: Binary 节点以 per-op NodeType 标识具体运算。
    private bool IsBinaryNode(Expression expr) {
        if (expr.NodeType == ExpressionType.Add) { return true; }
        if (expr.NodeType == ExpressionType.Subtract) { return true; }
        if (expr.NodeType == ExpressionType.Multiply) { return true; }
        if (expr.NodeType == ExpressionType.Divide) { return true; }
        if (expr.NodeType == ExpressionType.Modulo) { return true; }
        if (expr.NodeType == ExpressionType.Equal) { return true; }
        if (expr.NodeType == ExpressionType.NotEqual) { return true; }
        if (expr.NodeType == ExpressionType.LessThan) { return true; }
        if (expr.NodeType == ExpressionType.LessThanOrEqual) { return true; }
        if (expr.NodeType == ExpressionType.GreaterThan) { return true; }
        if (expr.NodeType == ExpressionType.GreaterThanOrEqual) { return true; }
        if (expr.NodeType == ExpressionType.AndAlso) { return true; }
        if (expr.NodeType == ExpressionType.OrElse) { return true; }
        return false;
    }

    /// RFC 039 Sprint 2d Slice 2: Unary 节点以 per-op NodeType 标识具体运算。
    private bool IsUnaryNode(Expression expr) {
        if (expr.NodeType == ExpressionType.Not) { return true; }
        if (expr.NodeType == ExpressionType.Negate) { return true; }
        return false;
    }

    /// 方法调用翻译——查询链节点（Table/Where/Select/OrderBy/OrderByDescending/ToList）。
    public string TranslateMethodCall(Expression expr) {
        string method = expr.GetMethodName();
        Expression target = expr.GetTarget();
        Expression arg0 = expr.GetArg0();

        if (method == "Table") {
            // 根: 表名（Arg0 为 ConstantExpression，值为表名字符串）
            if (arg0 != null) {
                return this.TranslateNode(arg0);
            }
            return "";
        }
        if (method == "ToList") {
            // 触发翻译：若 Target 链无 Select，补 "SELECT * FROM "
            string inner = "";
            if (target != null) {
                inner = this.TranslateNode(target);
            }
            if (target != null && this.NeedsSelect(target)) {
                return "SELECT * FROM " + inner;
            }
            return inner;
        }
        if (method == "Where") {
            string t = "";
            if (target != null) { t = this.TranslateNode(target); }
            string pred = "";
            if (arg0 != null) { pred = this.TranslateNode(arg0); }
            return t + " WHERE " + pred;
        }
        if (method == "Select") {
            string selector = "";
            if (arg0 != null) { selector = this.TranslateNode(arg0); }
            string t = "";
            if (target != null) { t = this.TranslateNode(target); }
            return "SELECT " + selector + " FROM " + t;
        }
        if (method == "OrderBy" || method == "OrderByDescending") {
            return this.TranslateOrderBy(expr);
        }
        return "";
    }

    /// ORDER BY 翻译：收集链式 OrderBy/OrderByDescending 的所有 key（外层主键在前），
    /// 输出单条 `ORDER BY k1 [ASC|DESC], k2 [ASC|DESC]`——对齐 C# LINQ 链式 OrderBy
    /// 语义（外层最后应用为主键，稳定排序下内层为次级）。
    public string TranslateOrderBy(Expression expr) {
        string keys = "";
        Expression cur = expr;
        while (cur != null && cur.NodeType == ExpressionType.Call) {
            string m = cur.GetMethodName();
            if (m != "OrderBy" && m != "OrderByDescending") { break; }
            string key = "";
            Expression arg0 = cur.GetArg0();
            if (arg0 != null) { key = this.TranslateNode(arg0); }
            if (m == "OrderByDescending") { key = key + " DESC"; }
            if (keys == "") { keys = key; } else { keys = keys + ", " + key; }
            cur = cur.GetTarget();
        }
        string t = "";
        if (cur != null) { t = this.TranslateNode(cur); }
        return t + " ORDER BY " + keys;
    }

    /// 判断查询链是否需要补 "SELECT * FROM"。
    /// Table 根节点需要；Select 自身已含 SELECT 不需要；
    /// Where/OrderBy/OrderByDescending 继承自 Target。
    public bool NeedsSelect(Expression expr) {
        if (expr.NodeType != ExpressionType.Call) {
            return false;
        }
        string method = expr.GetMethodName();
        if (method == "Table") {
            return true;
        }
        if (method == "Select") {
            return false;
        }
        if (method == "Where" || method == "OrderBy" || method == "OrderByDescending") {
            Expression target = expr.GetTarget();
            if (target != null) {
                return this.NeedsSelect(target);
            }
            return false;
        }
        return false;
    }

    /// 二元运算翻译：Left OP Right，按 per-op NodeType 映射 SQL 运算符。
    public string TranslateBinary(Expression expr) {
        string op = "";
        if (expr.NodeType == ExpressionType.Equal) { op = "="; }
        else if (expr.NodeType == ExpressionType.NotEqual) { op = "<>"; }
        else if (expr.NodeType == ExpressionType.AndAlso) { op = "AND"; }
        else if (expr.NodeType == ExpressionType.OrElse) { op = "OR"; }
        else if (expr.NodeType == ExpressionType.GreaterThan) { op = ">"; }
        else if (expr.NodeType == ExpressionType.GreaterThanOrEqual) { op = ">="; }
        else if (expr.NodeType == ExpressionType.LessThan) { op = "<"; }
        else if (expr.NodeType == ExpressionType.LessThanOrEqual) { op = "<="; }
        else if (expr.NodeType == ExpressionType.Add) { op = "+"; }
        else if (expr.NodeType == ExpressionType.Subtract) { op = "-"; }
        else if (expr.NodeType == ExpressionType.Multiply) { op = "*"; }
        else if (expr.NodeType == ExpressionType.Divide) { op = "/"; }
        else if (expr.NodeType == ExpressionType.Modulo) { op = "%"; }

        Expression left = expr.GetLeft();
        Expression right = expr.GetRight();
        string leftSql = "";
        string rightSql = "";
        if (left != null) { leftSql = this.TranslateNode(left); }
        if (right != null) { rightSql = this.TranslateNode(right); }
        return leftSql + " " + op + " " + rightSql;
    }

    /// 一元运算翻译：OP Operand，按 per-op NodeType 映射 SQL 运算符。
    public string TranslateUnary(Expression expr) {
        string op = "";
        if (expr.NodeType == ExpressionType.Not) { op = "NOT"; }
        else if (expr.NodeType == ExpressionType.Negate) { op = "-"; }
        Expression operand = expr.GetOperand();
        string operandSql = "";
        if (operand != null) { operandSql = this.TranslateNode(operand); }
        return op + " " + operandSql;
    }

    /// 常量翻译：字符串加单引号，数值/布尔直接输出。
    public string TranslateConstant(Expression expr) {
        if (expr.IsStringConstant()) {
            return "'" + expr.GetStringValue() + "'";
        }
        return expr.GetStringValue();
    }

    /// 三元条件翻译：CASE WHEN <cond> THEN <then> ELSE <else> END。
    public string TranslateConditional(Expression expr) {
        Expression cond = expr.GetCond();
        Expression thenE = expr.GetThen();
        Expression elseE = expr.GetElse();
        string condSql = "";
        string thenSql = "";
        string elseSql = "";
        if (cond != null) { condSql = this.TranslateNode(cond); }
        if (thenE != null) { thenSql = this.TranslateNode(thenE); }
        if (elseE != null) { elseSql = this.TranslateNode(elseE); }
        return "CASE WHEN " + condSql + " THEN " + thenSql + " ELSE " + elseSql + " END";
    }

    /// 类型转换翻译：CAST(<operand> AS <target_type>)。
    /// Arc 类型名按 SQL 方言映射（int→INTEGER, string→TEXT, double→REAL）。
    public string TranslateCast(Expression expr) {
        Expression operand = expr.GetExpr();
        string target = expr.GetTargetType();
        string operandSql = "";
        if (operand != null) { operandSql = this.TranslateNode(operand); }
        string sqlType = this.MapSqlType(target);
        return "CAST(" + operandSql + " AS " + sqlType + ")";
    }

    /// Arc 类型名 → SQLite 类型映射。
    public string MapSqlType(string arcType) {
        if (arcType == "int" || arcType == "long") { return "INTEGER"; }
        if (arcType == "string") { return "TEXT"; }
        if (arcType == "double" || arcType == "float") { return "REAL"; }
        if (arcType == "bool") { return "INTEGER"; }
        return arcType;
    }
}

namespace Arc.Linq.Expressions {
    using Arc.Collections;

    /// LINQ comprehension 表达式——对应 Arc AST `Expr::Query(QueryExpr)`。
    ///
    /// RFC 022 §2.2.10 L2 扩展节点。表示 `from x in xs where ... select ...` 形式的
    /// LINQ 查询表达式。typeck 通常将其脱糖为方法调用链（Where/Select/OrderBy 等），
    /// 但 QueryExpression 节点保留原始语法结构供 Source Generator 等场景分析。
    ///
    /// Clauses 为查询子句列表（From/Let/Where/OrderBy/Join/GroupBy），
    /// Selector 为最终投影表达式（`select ...` 子句）。
    ///
    /// 与 IQueryable 方法调用链的区别：
    /// - 方法调用链：`db.Users.Where(u => ...).Select(u => ...)`——已脱糖
    /// - QueryExpression：`from u in db.Users where ... select ...`——保留原始结构
    ///
    /// 不进入 ORM 查询翻译路径——SqlTranslator 遇到此节点报「不可翻译」错误
    /// （查询已在 typeck 脱糖为方法调用链，不应出现 QueryExpression 节点）。
    ///
    /// 注：属性名 `Selector`（而非 `Select`）——`select` 在 Arc 中是 LINQ 查询
    /// 表达式的硬保留字，不可作为标识符使用。`Selector` 表达相同语义（投影选择器）。
    public class QueryExpression : Expression {
        public List<Expression> Clauses { get; }
        public Expression Selector { get; }

        public QueryExpression(List<Expression> clauses, Expression selector) {
            NodeType = ExpressionType.Query;
            Clauses = clauses;
            Selector = selector;
        }
    }
}

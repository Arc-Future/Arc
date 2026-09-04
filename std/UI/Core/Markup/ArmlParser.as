// RFC 037 §10 AI 原生：ArmlParser — 运行时 ARML 字符串 → Element 树解析器。
//
// 设计原则：
//   1. 简单但健壮——覆盖 ARML 常用子集（元素/属性/嵌套/文本/自闭合）
//   2. 错误容忍——未知类型 fallback 到 Element，解析错误返回已构建的部分树
//   3. 单一职责——只负责解析与实例化，不涉及校验/渲染
//   4. 与编译期 arc-ui parser 同构但不重复——运行时简化版，
//      编译期完整校验仍由 arc-ui typeck 承担
//
// 支持语法：
//   <Type>...</Type>                     开-闭标签
//   <Type attr="val"/>                   自闭合+属性
//   <Type x:Name="foo">...</Type>        命名空间前缀属性
//   <Type>text content</Type>            文本内容
//   <!-- comment -->                      XML 注释（跳过）
//   <?xml version="1.0"?>                XML 声明（跳过）
//
// 不支持（首版）：
//   {x:Bind ...} 标记扩展（运行时无绑定上下文）
//   {StaticResource ...} 资源引用
//   属性元素 <Button.Background>
//   指令元素 <Style>/<ResourceDictionary>

namespace Arc.UI.Markup;

using Arc.Collections;
using Arc.Text;
using Arc.UI;
using Arc.UI.Media;

/// <summary>
/// ARML 运行时解析结果。
/// </summary>
public class ArmlParseResult {
    /// <summary>解析出的根元素（null 表示解析失败）。</summary>
    public Element Root;

    /// <summary>解析诊断信息（错误/警告）。</summary>
    public List<string> Diagnostics;

    /// <summary>是否解析成功（无致命错误）。</summary>
    public bool Success;
}

/// <summary>
/// 运行时 ARML 解析器——将 ARML 字符串解析为 Element 树。
/// </summary>
public class ArmlParser {
    private string _source;
    private int _pos;
    private int _len;
    private IElementFactory _factory;
    private List<string> _diagnostics;

    /// <summary>
    /// 解析 ARML 字符串为 Element 树。
    /// </summary>
    /// <param name="arml">ARML 字符串。</param>
    /// <param name="factory">元素工厂（null 用默认工厂）。</param>
    /// <returns>解析结果（含根元素与诊断信息）。</returns>
    public static ArmlParseResult Parse(string arml, IElementFactory factory) {
        ArmlParser parser = new ArmlParser();
        return parser.DoParse(arml, factory);
    }

    /// <summary>
    /// 解析 ARML 字符串为 Element 树（使用默认工厂）。
    /// </summary>
    public static ArmlParseResult Parse(string arml) {
        return Parse(arml, null);
    }

    private ArmlParseResult DoParse(string arml, IElementFactory factory) {
        ArmlParseResult result = new ArmlParseResult();
        result.Diagnostics = new List<string>();
        result.Success = false;
        result.Root = null;

        if (arml == null || arml.Length == 0) {
            result.Diagnostics.Add("ARML 源为空");
            return result;
        }

        _source = arml;
        _pos = 0;
        _len = arml.Length;
        _diagnostics = result.Diagnostics;
        _factory = factory;
        if (_factory == null) {
            _factory = new DefaultElementFactory();
        }

        // 跳过 prolog（XML 声明、注释、空白）
        this.SkipProlog();

        if (_pos >= _len) {
            result.Diagnostics.Add("ARML 源仅包含 prolog，无根元素");
            return result;
        }

        // 必须以 '<' 开头
        if (!this.PeekChar('<')) {
            result.Diagnostics.Add("期望根元素 '<' 起始标签");
            return result;
        }

        Element root = this.ParseElementCore();
        if (root == null) {
            return result;
        }

        result.Root = root;
        result.Success = true;
        return result;
    }

    // ===== 词法辅助 =====

    private char Peek() {
        if (_pos >= _len) {
            return '\0';
        }
        return _source[_pos];
    }

    private char PeekAt(int offset) {
        int p = _pos + offset;
        if (p >= _len) {
            return '\0';
        }
        return _source[p];
    }

    private bool PeekChar(char c) {
        return this.Peek() == c;
    }

    private void Advance() {
        if (_pos < _len) {
            _pos++;
        }
    }

    private void AdvanceBy(int n) {
        _pos = _pos + n;
        if (_pos > _len) {
            _pos = _len;
        }
    }

    private bool StartsWith(string s) {
        int sl = s.Length;
        if (_pos + sl > _len) {
            return false;
        }
        for (int i = 0; i < sl; i++) {
            if (_source[_pos + i] != s[i]) {
                return false;
            }
        }
        return true;
    }

    private void SkipWhitespace() {
        while (_pos < _len) {
            char c = _source[_pos];
            if (c == ' ' || c == '\t' || c == '\r' || c == '\n') {
                _pos++;
            } else {
                break;
            }
        }
    }

    // ===== Prolog 跳过 =====

    private void SkipProlog() {
        // BOM
        if (this.StartsWith("\uFEFF")) {
            _pos = _pos + 3;
        }
        while (_pos < _len) {
            this.SkipWhitespace();
            if (this.StartsWith("<?xml")) {
                // 跳过 XML 声明
                while (_pos < _len && !this.StartsWith("?>")) {
                    _pos++;
                }
                if (this.StartsWith("?>")) {
                    _pos = _pos + 2;
                }
                continue;
            }
            if (this.StartsWith("<!--")) {
                this.SkipComment();
                continue;
            }
            break;
        }
    }

    private void SkipComment() {
        if (!this.StartsWith("<!--")) {
            return;
        }
        _pos = _pos + 4;
        while (_pos < _len) {
            if (this.StartsWith("-->")) {
                _pos = _pos + 3;
                return;
            }
            _pos++;
        }
        // 未找到闭合，静默跳过到末尾
        _pos = _len;
    }

    // ===== 元素解析 =====

    /// <summary>
    /// 解析元素：<Name attrs>content</Name> 或 <Name attrs/>。
    /// 前置 '<' 已在调用方确认。
    /// </summary>
    private Element ParseElementCore() {
        // 消费 '<'
        this.Advance();

        // 跳过可能的属性元素前缀（不支持，简化处理）
        // 解析限定名
        string typeName = this.ParseQName();
        if (typeName == null || typeName.Length == 0) {
            _diagnostics.Add("元素名不能为空");
            return null;
        }

        // 解析属性
        List<AttrEntry> attrs = this.ParseAttributes();

        // 跳过空白
        this.SkipWhitespace();

        // 检查自闭合或结束
        bool selfClosing = false;
        if (this.StartsWith("/>")) {
            selfClosing = true;
            _pos = _pos + 2;
        } else if (this.PeekChar('>')) {
            this.Advance();
        } else {
            _diagnostics.Add("期望 '>' 或 '/>' 结束标签 <" + typeName + ">");
            return null;
        }

        // 创建元素实例
        Element element = this.CreateElement(typeName);
        if (element == null) {
            // 未知类型 → fallback 到 Element
            element = new Element();
            element.TypeName = typeName;
            _diagnostics.Add("未知元素类型 <" + typeName + ">，fallback 到 Element");
        }

        // 应用属性
        this.ApplyAttributes(element, attrs);

        if (!selfClosing) {
            // 解析内容（子元素 + 文本）
            this.ParseElementContent(element, typeName);
        }

        // 触发 OnLoaded（模拟挂载生命周期）
        element.OnLoaded();
        return element;
    }

    /// <summary>
    /// 解析限定名 "prefix:local" 或 "local"。
    /// </summary>
    private string ParseQName() {
        int start = _pos;
        while (_pos < _len) {
            char c = _source[_pos];
            if (c == ':' || c == '.' ||
                (c >= 'a' && c <= 'z') ||
                (c >= 'A' && c <= 'Z') ||
                (c >= '0' && c <= '9' && _pos > start) ||
                c == '_') {
                _pos++;
            } else {
                break;
            }
        }
        if (_pos == start) {
            return null;
        }
        string name = _source.Substring(start, _pos - start);
        // 去掉命名空间前缀（运行时不需要前缀）
        int colonIdx = name.IndexOf(':');
        if (colonIdx >= 0) {
            name = name.Substring(colonIdx + 1);
        }
        return name;
    }

    // ===== 属性解析 =====

    private List<AttrEntry> ParseAttributes() {
        List<AttrEntry> attrs = new List<AttrEntry>();
        while (_pos < _len) {
            this.SkipWhitespace();
            char c = this.Peek();
            if (c == '>' || c == '/' || c == '\0') {
                break;
            }
            // 跳过 xmlns 声明
            string lookAhead = this.PeekAheadName();
            if (lookAhead == "xmlns" || lookAhead == "xmlns:x") {
                this.SkipXmlnsDeclaration();
                continue;
            }

            AttrEntry attr = this.ParseOneAttribute();
            if (attr != null) {
                attrs.Add(attr);
            } else {
                // 解析失败，跳过一个字符避免死循环
                this.Advance();
            }
        }
        return attrs;
    }

    private string PeekAheadName() {
        int start = _pos;
        while (_pos < _len) {
            char c = _source[_pos];
            if ((c >= 'a' && c <= 'z') || (c >= 'A' && c <= 'Z') || c == ':' || c == '_') {
                _pos++;
            } else {
                break;
            }
        }
        if (_pos == start) {
            return "";
        }
        string name = _source.Substring(start, _pos - start);
        _pos = start; // 回退
        return name;
    }

    private void SkipXmlnsDeclaration() {
        // 跳过 xmlns="..." 或 xmlns:x="..."
        while (_pos < _len) {
            char c = _source[_pos];
            if (c == '"') {
                // 跳过引号内内容
                _pos++;
                while (_pos < _len && _source[_pos] != '"') {
                    _pos++;
                }
                if (_pos < _len) {
                    _pos++;
                }
                return;
            }
            _pos++;
        }
    }

    private AttrEntry ParseOneAttribute() {
        AttrEntry attr = new AttrEntry();
        attr.IsPropertyElement = false;

        // 解析属性名（可能含前缀 x:Name 或附加属性 Grid.Row）
        int nameStart = _pos;
        while (_pos < _len) {
            char c = _source[_pos];
            if ((c >= 'a' && c <= 'z') || (c >= 'A' && c <= 'Z') ||
                (c >= '0' && c <= '9' && _pos > nameStart) ||
                c == ':' || c == '.' || c == '_') {
                _pos++;
            } else {
                break;
            }
        }
        if (_pos == nameStart) {
            return null;
        }
        string rawName = _source.Substring(nameStart, _pos - nameStart);

        // 处理带前缀的属性名：x:Name → Name，Grid.Row → Row
        // 前缀在运行时无意义（x:Name 不参与 DP 解析）
        int colonIdx = rawName.IndexOf(':');
        if (colonIdx >= 0) {
            attr.Name = rawName.Substring(colonIdx + 1);
        } else {
            attr.Name = rawName;
        }

        this.SkipWhitespace();
        if (!this.PeekChar('=')) {
            _diagnostics.Add("属性 '" + rawName + "' 缺少 '='");
            return null;
        }
        this.Advance(); // 消费 '='
        this.SkipWhitespace();

        attr.Value = this.ParseAttributeValue();
        if (attr.Value == null) {
            return null;
        }
        return attr;
    }

    private string ParseAttributeValue() {
        if (!this.PeekChar('"')) {
            _diagnostics.Add("期望 '\"' 开始属性值");
            return null;
        }
        this.Advance(); // 消费开头 '"'

        // 检查标记扩展 "{...}"
        if (this.PeekChar('{')) {
            // 跳过标记扩展直到 '}'（运行时不支持，但不破坏解析）
            while (_pos < _len) {
                char c = _source[_pos];
                if (c == '}') {
                    this.Advance();
                    this.SkipWhitespace();
                    if (this.PeekChar('"')) {
                        this.Advance();
                    }
                    return ""; // 标记扩展值返回空串
                }
                this.Advance();
            }
            _diagnostics.Add("标记扩展未闭合 '}'");
            return "";
        }

        // 普通字面量
        int start = _pos;
        while (_pos < _len) {
            char c = _source[_pos];
            if (c == '"') {
                string val = _source.Substring(start, _pos - start);
                this.Advance(); // 消费结尾 '"'
                return val;
            }
            if (c == '\0') {
                break;
            }
            _pos++;
        }
        _diagnostics.Add("属性值未闭合 '\"'");
        return _source.Substring(start, _pos - start);
    }

    // ===== 元素内容解析 =====

    private void ParseElementContent(Element parent, string parentTypeName) {
        List<string> textParts = new List<string>();
        bool hasChildElements = false;

        while (_pos < _len) {
            this.SkipWhitespace();

            // 检查闭合标签
            if (this.StartsWith("</")) {
                this.AdvanceBy(2); // 消费 '</'
                string closeName = this.ParseQName();
                this.SkipWhitespace();
                if (this.PeekChar('>')) {
                    this.Advance();
                }
                // 检查闭合标签名是否匹配（不匹配为警告但不中断）
                if (closeName != null && closeName != parentTypeName) {
                    _diagnostics.Add("闭合标签 </" + closeName + "> 与开启标签 <" + parentTypeName + "> 不匹配");
                }

                // 处理累积的文本内容
                this.FlushTextContent(parent, textParts, hasChildElements);
                return;
            }

            // 注释
            if (this.StartsWith("<!--")) {
                this.SkipComment();
                continue;
            }

            // 子元素
            if (this.PeekChar('<')) {
                // 检查是否为属性元素语法 <Parent.Property>
                // 运行时简化：跳过属性元素
                int savePos = _pos;
                this.Advance();
                string checkName = this.ParseQName();
                _pos = savePos;

                if (checkName != null && this.IsPropertyElementSyntax(checkName, parentTypeName)) {
                    // 属性元素：跳过其内容（运行时不支持）
                    this.SkipPropertyElement(checkName);
                    continue;
                }

                // 普通子元素
                if (!hasChildElements && textParts.Count > 0) {
                    // 有文本 + 子元素混合——将文本设为 Content
                    string text = this.JoinTextParts(textParts);
                    textParts.Clear();
                    this.SetTextContent(parent, text);
                }
                hasChildElements = true;

                Element child = this.ParseElementCore();
                if (child != null) {
                    parent.AddChild(child);
                }
                continue;
            }

            // 文本内容
            int textStart = _pos;
            while (_pos < _len && !this.PeekChar('<')) {
                char c = _source[_pos];
                if (c == '<') {
                    break;
                }
                _pos++;
            }
            string text = _source.Substring(textStart, _pos - textStart);
            // 如果文本全是空白且无子元素，保留空白文本
            // 规范化：去除首尾空白（但保留中间空白）
            string trimmed = text.Trim();
            if (trimmed.Length > 0) {
                textParts.Add(text);
            }
        }

        // 未找到闭合标签——警告
        _diagnostics.Add("元素 <" + parentTypeName + "> 未找到闭合标签");
        this.FlushTextContent(parent, textParts, hasChildElements);
    }

    private bool IsPropertyElementSyntax(string name, string parentName) {
        int dot = name.IndexOf('.');
        if (dot < 0) {
            return false;
        }
        string owner = name.Substring(0, dot);
        return owner == parentName;
    }

    private void SkipPropertyElement(string propName) {
        // 消费 '<'
        this.Advance();
        // 消费属性名
        this.ParseQName();
        // 跳过属性
        this.ParseAttributes();
        this.SkipWhitespace();

        // 检查自闭合
        if (this.StartsWith("/>")) {
            this.AdvanceBy(2);
            return;
        }
        if (this.PeekChar('>')) {
            this.Advance();
        }

        // 跳过内容直到闭合 </PropertyName>
        int depth = 1;
        while (_pos < _len && depth > 0) {
            this.SkipWhitespace();
            if (this.StartsWith("</")) {
                this.AdvanceBy(2);
                this.ParseQName();
                this.SkipWhitespace();
                if (this.PeekChar('>')) {
                    this.Advance();
                }
                depth--;
            } else if (this.PeekChar('<')) {
                this.Advance();
                string childName = this.ParseQName();
                this.SkipWhitespace();
                if (this.StartsWith("/>")) {
                    this.AdvanceBy(2);
                } else if (this.PeekChar('>')) {
                    this.Advance();
                    depth++;
                }
            } else {
                this.Advance();
            }
        }
    }

    private void FlushTextContent(Element parent, List<string> textParts, bool hasChildElements) {
        if (textParts.Count == 0) {
            return;
        }
        string text = this.JoinTextParts(textParts);
        textParts.Clear();
        if (!hasChildElements) {
            this.SetTextContent(parent, text);
        }
    }

    /// <summary>
    /// 安全设置文本内容——TextBlock 持 Text DP 直落 Text；
    /// 其余仅 ContentControl（Button 等）承载 Content.Text；
    /// 纯容器（Panel 派生）与未知 fallback 的 Element 无文本承载能力，静默跳过。
    /// </summary>
    private void SetTextContent(Element element, string text) {
        if (element == null) {
            return;
        }
        TextBlock tb = null;
        if (element is TextBlock) {
            tb = (TextBlock)element;
        }
        if (tb != null) {
            tb.Text = text;
            return;
        }
        ContentControl cc = null;
        if (element is ContentControl) {
            cc = (ContentControl)element;
        }
        if (cc != null) {
            cc.Content = Content.Text(text);
        }
    }

    private string JoinTextParts(List<string> parts) {
        if (parts.Count == 0) {
            return "";
        }
        if (parts.Count == 1) {
            return parts[0];
        }
        StringBuilder sb = new StringBuilder();
        for (int i = 0; i < parts.Count; i++) {
            sb.Append(parts[i]);
        }
        return sb.ToString();
    }

    // ===== 属性应用 =====

    private void ApplyAttributes(Element element, List<AttrEntry> attrs) {
        string typeName = element.TypeName;
        for (int i = 0; i < attrs.Count; i++) {
            AttrEntry attr = attrs[i];
            this.ApplyOneAttribute(element, typeName, attr);
        }
    }

    private void ApplyOneAttribute(Element element, string typeName, AttrEntry attr) {
        string name = attr.Name;
        string value = attr.Value;

        if (name == null || value == null) {
            return;
        }

        // 附加属性：Grid.Row / Canvas.Left / DockPanel.Dock 等。
        // 去掉 owner 前缀（"Grid.Row" → "Row"）与本地布局键匹配；
        // 同时保留完整键（"Grid.Row"）供 owner 语义扩展，双写便于读取方选择。
        string localName = name;
        int dotIdx = name.IndexOf('.');
        if (dotIdx >= 0) {
            localName = name.Substring(dotIdx + 1);
        }

        // x:Name → 设置 Element.Name
        if (localName == "Name") {
            element.Name = value;
            return;
        }

        // Row / Column → 数值附加属性
        if (localName == "Row" || localName == "Column") {
            double numVal = 0.0;
            double.TryParse(value, out numVal);
            element.SetAttachedNumber(name, numVal);
            element.SetAttachedNumber(localName, numVal);
            return;
        }

        // Dock → 字符串附加属性
        if (localName == "Dock") {
            element.SetAttachedString(name, value);
            element.SetAttachedString(localName, value);
            return;
        }

        // Left / Top → 数值附加属性
        if (localName == "Left" || localName == "Top") {
            double numVal = 0.0;
            double.TryParse(value, out numVal);
            element.SetAttachedNumber(name, numVal);
            element.SetAttachedNumber(localName, numVal);
            return;
        }

        // Width / Height → FrameworkElement DP
        if (localName == "Width" || localName == "Height") {
            double numVal = 0.0;
            double.TryParse(value, out numVal);
            FrameworkElement fe = null;
            if (element is FrameworkElement) {
                fe = (FrameworkElement)element;
            }
            if (fe != null) {
                if (localName == "Width") {
                    fe.Width = numVal;
                } else {
                    fe.Height = numVal;
                }
                return;
            }
        }

        // 尝试按 DP 名解析并设置（统一走 DpValueConverter 单一事实来源）
        object dpObj = element.ResolveProperty(localName);
        if (dpObj != null) {
            DpValueConverter.SetValue(element, dpObj, value);
            return;
        }

        // 未知属性静默跳过（可能是仅编译期属性）
    }

    // ===== 元素创建 =====

    private Element CreateElement(string typeName) {
        if (_factory == null) {
            return null;
        }
        Element element = _factory.Create(typeName);
        if (element != null) {
            // 确保 TypeName 正确（某些构造函数可能未设置）
            if (element.TypeName == null || element.TypeName.Length == 0) {
                element.TypeName = typeName;
            }
        }
        return element;
    }
}

/// <summary>
/// 属性解析条目——单个属性的名/值（与属性元素语法标记）。
/// 顶层类承载：Arc 不支持嵌套类，属性解析仅由 ArmlParser 内部使用。
/// </summary>
internal class AttrEntry {
    public string Name;
    public string Value;
    public bool IsPropertyElement; // true 表示来自属性元素语法
}

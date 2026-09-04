// RFC 009 M5-1: Source Generator interface and context types (D13.3).
// Phase 2: Extended with TypeTable and AttributeList.GetArg for serialization.
//
// This file defines the M5 Source Generator system's interfaces and context types:
//   - IGenerator: Interface that Source Generators must implement
//   - GeneratorContext: Compile-time accessible global information context
//   - AttributeTable / AttributeList / SymbolTable / TypeTable: Compile-time placeholder types
//
// Design notes (RFC D13.3):
//   - Classes marked with [SourceGenerator] must implement IGenerator
//   - The compiler calls IGenerator.Generate(GeneratorContext) in Pass 3
//   - Generate returns List<string>, each string parsed as an independent Arc source file
//   - Generate method body executed by restricted evaluator (shared D10.2 whitelist with M4)
//
// Compile-time placeholder types:
//   These types are declared as regular classes in Arc source, but method bodies
//   are placeholder implementations (returning default values). The compiler
//   intercepts method calls on these types in Pass 3 and injects real compile-time data.
//
// Architecture redline (RFC 009 D13.6):
//   - M5 shares the restricted evaluator and whitelist with M4
//   - M5 generated code spans point to Generate method locations (D10.4 same mechanism)
//   - M5 does not introduce cross-TU / incremental / parallel / caching (D13.7 non-goals)

namespace Arc.CodeGeneration;

using Arc.Collections;

/// <summary>
/// Compile-time attribute list placeholder type.
///
/// Represents all attributes attached to a symbol. The restricted evaluator
/// intercepts Has / GetArg / GetArgCount / GetNamedArg calls in Pass 3
/// and returns real attribute data.
/// </summary>
public class AttributeList {
    public bool Has(string name) {
        return false;
    }

    public string GetArg(string attrName, int index) {
        return "";
    }

    public int GetArgCount() {
        return 0;
    }

    public string GetNamedArg(string attrName, string argName) {
        return "";
    }
}

/// <summary>
/// Compile-time attribute table placeholder type (RFC 012 D13.3).
///
/// Provides query interface for the global attribute table. The restricted
/// evaluator intercepts method calls in Pass 3 and injects real compile-time
/// attribute data (based on typeck output AttributeTable).
/// </summary>
public class AttributeTable {
    public int Count { get; }

    public int GetDefIdAt(int index) {
        return 0;
    }

    public AttributeList GetAttrs(int defId) {
        return new AttributeList();
    }
}

/// <summary>
/// Compile-time symbol table placeholder type (RFC 012 D13.3).
///
/// Provides symbol metadata query interface. The restricted evaluator
/// intercepts method calls in Pass 3 and injects real compile-time symbol data.
/// </summary>
public class SymbolTable {
    public string GetTypeName(int defId) {
        return "";
    }

    public string GetMemberName(int defId) {
        return "";
    }
}

/// <summary>
/// Phase 2: Compile-time type table placeholder type.
///
/// Provides type member metadata query interface for serialization source
/// generators. The restricted evaluator intercepts method calls in Pass 3
/// and injects real type metadata (field names, types, base class).
/// </summary>
public class TypeTable {
    public string GetTypeName(int defId) {
        return "";
    }

    public string GetKind(int defId) {
        return "";
    }

    public int GetFieldCount(int defId) {
        return 0;
    }

    public string GetFieldName(int defId, int index) {
        return "";
    }

    public string GetFieldType(int defId, int index) {
        return "";
    }

    public string GetBaseType(int defId) {
        return "";
    }
}

/// <summary>
/// Source Generator call context (RFC 009 D13.3).
///
/// Compile-time accessible global information constructed by the compiler
/// in Pass 3 and passed to IGenerator.Generate. Source generators query
/// the global attribute table, symbol table, and type table to generate
/// new code.
/// </summary>
public class GeneratorContext {
    public AttributeTable Attributes { get; }
    public SymbolTable Symbols { get; }
    public List<string> SourceFiles { get; }
    public TypeTable TypeTable { get; }

    public GeneratorContext() {
        Attributes = new AttributeTable();
        Symbols = new SymbolTable();
        SourceFiles = new List<string>();
        TypeTable = new TypeTable();
    }
}

/// <summary>
/// Interface that Source Generators must implement (RFC 009 D13.3).
///
/// Classes marked with [SourceGenerator] must implement this interface.
/// The compiler calls Generate in Pass 3; each string in the returned
/// list is parsed as an independent Arc source file and appended to the
/// current compilation unit.
/// </summary>
public interface IGenerator {
    List<string> Generate(GeneratorContext context);
}

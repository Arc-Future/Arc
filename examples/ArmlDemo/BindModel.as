// Nested Binding Model.Name leaf. Name notify is the live OneWay channel;
// MainWindow.Model is the replaceable parent (rebind on assign).

namespace ArmlDemo;

using Arc.ComponentModel;

public class BindModel {
    [Observable] public string Name { get; set; }
}

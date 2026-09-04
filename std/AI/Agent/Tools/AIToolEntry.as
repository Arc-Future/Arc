// RFC 038: linked tool entry — Handler is class-typed field (Host._provider pattern).
namespace Arc.Agent;

/// <summary>One registered tool node. Next forms a singly-linked registry (avoids List&lt;interface&gt; AV).</summary>
internal class AIToolEntry {
    public AIToolDescriptor Descriptor;
    public AIToolHandler Handler;
    public AIToolEntry Next;

    public AIToolEntry() {
        this.Descriptor = null;
        this.Handler = null;
        this.Next = null;
    }

    public AIToolEntry(AIToolDescriptor descriptor, AIToolHandler handler) {
        this.Descriptor = descriptor;
        this.Handler = handler;
        this.Next = null;
    }
}

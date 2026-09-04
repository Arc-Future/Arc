namespace Arc.Agent;
internal class AISessionBudget {
    public int MaxTurns;
    public int MaxMessages;
    public int TurnsUsed;
    public int MessagesUsed;
    public AISessionBudget() {
        this.MaxTurns = 16; this.MaxMessages = 128;
        this.TurnsUsed = 0; this.MessagesUsed = 0;
    }
    public bool CanStartTurn() {
        if (this.MaxTurns <= 0) { return true; }
        return this.TurnsUsed < this.MaxTurns;
    }
    public bool CanAddMessages(int count) {
        if (this.MaxMessages <= 0) { return true; }
        return this.MessagesUsed + count <= this.MaxMessages;
    }
}
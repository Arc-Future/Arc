namespace UnitTest.Core.MultiFile;

public class GreetingService {
    private string _prefix;

    public GreetingService(string prefix) {
        _prefix = prefix;
    }

    public string Greet(string name) {
        return _prefix + name;
    }

    public static string DefaultMessage {
        get {
            return "Hello from another file!";
        }
    }
}

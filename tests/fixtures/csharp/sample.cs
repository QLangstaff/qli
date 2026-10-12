// TODO: replace the stub transport
namespace Sample;

public class Widget
{
    /* FIXME: block comments count too */
    private const int DefaultRetries = 3;

    /// XXX: doc comments count too
    private static int Foo(int x)
    {
        return x + 1;
    }

    // HACK: temporary until the API settles
    public string Render()
    {
        return "TODO this string must NOT be flagged";
    }

    public static void Main()
    {
        Foo(DefaultRetries);
        Bar(); // Bar is undefined in this file
        new Widget().Render();
    }
}

namespace MKVOrchestrator.Core.Services;

/// <summary>
/// Case-insensitive comparer that orders embedded numbers by value, so
/// "Episode 2" sorts before "Episode 10".
/// </summary>
public sealed class NaturalStringComparer : IComparer<string>
{
    public static NaturalStringComparer Instance { get; } = new();

    public int Compare(string? x, string? y)
    {
        x ??= string.Empty;
        y ??= string.Empty;

        var xIndex = 0;
        var yIndex = 0;

        while (xIndex < x.Length && yIndex < y.Length)
        {
            var xChar = x[xIndex];
            var yChar = y[yIndex];

            if (char.IsDigit(xChar) && char.IsDigit(yChar))
            {
                var xStart = xIndex;
                var yStart = yIndex;

                while (xIndex < x.Length && char.IsDigit(x[xIndex])) xIndex++;
                while (yIndex < y.Length && char.IsDigit(y[yIndex])) yIndex++;

                var xNumberText = x[xStart..xIndex].TrimStart('0');
                var yNumberText = y[yStart..yIndex].TrimStart('0');

                xNumberText = xNumberText.Length == 0 ? "0" : xNumberText;
                yNumberText = yNumberText.Length == 0 ? "0" : yNumberText;

                var lengthCompare = xNumberText.Length.CompareTo(yNumberText.Length);
                if (lengthCompare != 0) return lengthCompare;

                var numberCompare = string.Compare(xNumberText, yNumberText, StringComparison.OrdinalIgnoreCase);
                if (numberCompare != 0) return numberCompare;

                continue;
            }

            var charCompare = char.ToUpperInvariant(xChar).CompareTo(char.ToUpperInvariant(yChar));
            if (charCompare != 0) return charCompare;

            xIndex++;
            yIndex++;
        }

        return x.Length.CompareTo(y.Length);
    }
}

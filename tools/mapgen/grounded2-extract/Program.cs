// Reads what the Grounded 2 map pack needs from the game's packaged files:
//
//   map.png    the in-game map's painting (4096 x 4096)
//   data.json  the map's world bounds, points of interest with their names, the
//              positions of landmark actors, and item and creature names
//
// Usage:
//   dotnet run -c Release -- --game <Grounded2 install> --mappings <.usmap> --oodle <oo2core_*_win64.dll> --out <dir>
//
// The game saves its packages without property names, so reading them needs type
// mappings: a development build of the agent writes them to %TEMP%\game-maps-unreal.usmap
// while the game runs (see docs/games/grounded-2.md). The packages are Oodle-compressed;
// any game's oo2core_*_win64.dll will do.

using System.Text.Json;
using CUE4Parse.Compression;
using CUE4Parse.FileProvider;
using CUE4Parse.MappingsProvider.Usmap;
using CUE4Parse.UE4.Assets.Exports;
using CUE4Parse.UE4.Assets.Exports.Texture;
using CUE4Parse.UE4.Objects.Core.Math;
using CUE4Parse.UE4.Objects.UObject;
using CUE4Parse.UE4.Versions;
using CUE4Parse_Conversion.Textures;

var opts = ParseArgs(args);
string Opt(string name) => opts.TryGetValue(name, out var v) ? v : throw new ArgumentException($"--{name} is required");

OodleHelper.Initialize(Opt("oodle"));
var provider = new DefaultFileProvider(Path.Combine(Opt("game"), "Augusta", "Content", "Paks"), SearchOption.TopDirectoryOnly, new VersionContainer(EGame.GAME_UE5_7), StringComparer.OrdinalIgnoreCase);
provider.Initialize();
provider.MappingsContainer = new FileUsmapTypeMappingsProvider(Opt("mappings"));
provider.Mount();
var outDir = Opt("out");
Directory.CreateDirectory(outDir);

const string MapTexture = "Augusta/Content/_Augusta/UI/Images/Map/T_UI_AugustaMainMap";
const string AbyssTexture = "Augusta/Content/Releases/Release_0_5_0/UI/Images/T_UI_AugustaAbyss";
const string GameInstance = "Augusta/Content/_Augusta/Global/GameInstance/BP_SurvivalGameInstance_Augusta";
const string MapPanel = "Augusta/Content/UI/Map/UI_MapPanel";
const string MainLevel = "Augusta/Content/_Augusta/Levels/Augusta_Main/Augusta_Main";
const string CellFolder = "Augusta/Content/_Augusta/Levels/Augusta_Main/Augusta_Main/_Generated_/";

// Actors worth a pin that are placed in the level, by class.
string[] landmarkClasses =
[
    "BP_Teleporter_C", "BP_Teleporter_Anywhere_C", "BP_BurgleStation_C", "BP_ResourceAnalyzer_Augusta_C",
    "BP_CommArray_simple_C", "BP_PlayerFallBackRespawn_C", "BP_DivingSpiderNest_C", "BP_Burgle_Quest_Flag_C",
];

// String table ids are assigned in the game's code. These were read from the running
// game (OEITextLibrary.GetLocStringText) and checked against the tables' contents.
var tableIds = new Dictionary<int, string>
{
    [1] = "game/gui", [2] = "game/items", [9] = "game/characters", [56] = "game/pointsofinterest", [79] = "game/areas",
};
var strings = LoadStrings();
string? Text(JsonElement loc)
{
    if (loc.ValueKind != JsonValueKind.Object) return null;
    if (loc.TryGetProperty("DefaultText", out var inner)) loc = inner;
    if (!loc.TryGetProperty("StringTableID", out var t) || !loc.TryGetProperty("StringID", out var s)) return null;
    return tableIds.TryGetValue(t.GetInt32(), out var table) && strings.TryGetValue(table, out var entries) && entries.TryGetValue(s.GetInt32(), out var text) ? text : null;
}

// --- map ------------------------------------------------------------------------------

void SaveTexture(string path, string file)
{
    var texture = provider.LoadPackageObject<UTexture2D>(path);
    using var bitmap = TextureDecoder.Decode(texture)!.ToSkBitmap();
    using var png = bitmap.Encode(SkiaSharp.SKEncodedImageFormat.Png, 100);
    File.WriteAllBytes(Path.Combine(outDir, file), png.ToArray());
}
SaveTexture(MapTexture, "map.png");
SaveTexture(AbyssTexture, "abyss.png");

static object Bounds(JsonElement levelData) => new
{
    minX = levelData.GetProperty("MinBounds").GetProperty("X").GetDouble(),
    minY = levelData.GetProperty("MinBounds").GetProperty("Y").GetDouble(),
    maxX = levelData.GetProperty("MaxBounds").GetProperty("X").GetDouble(),
    maxY = levelData.GetProperty("MaxBounds").GetProperty("Y").GetDouble(),
};

// The map's world rectangle: the main level's entry in the map panel's level list.
var bounds = Json(provider.LoadPackage(MapPanel).GetExports()).EnumerateArray()
    .SelectMany(e => e.TryGetProperty("Properties", out var p) ? Descendants(p) : [])
    .First(o => o.ValueKind == JsonValueKind.Object && o.TryGetProperty("DevLevel", out var level) && level.GetProperty("AssetPathName").GetString()!.Contains("Augusta_Main") && o.TryGetProperty("MinBounds", out _));
var mapBounds = Bounds(bounds);
// The Abyss, the region under the park, has its own map: the game instance's level list
// entry tagged `Level.Abyss`.
static bool HasBounds(JsonElement o) => o.ValueKind == JsonValueKind.Object && o.TryGetProperty("MinBounds", out _);
var abyssEntry = Json(provider.LoadPackage(GameInstance).GetExports()).EnumerateArray()
    .SelectMany(e => e.TryGetProperty("Properties", out var p) ? Descendants(p) : [])
    .Where(o => Descendants(o).Any(d => d.ValueKind == JsonValueKind.String && d.GetString() == "Level.Abyss") && Descendants(o).Count(HasBounds) == 1)
    .OrderBy(o => Descendants(o).Count())
    .First();
var abyssBounds = Bounds(Descendants(abyssEntry).First(HasBounds));

// --- points of interest and landmarks ---------------------------------------------------

var pois = new List<object>();
var landmarks = new List<object>();
var levels = provider.Files.Where(kv => kv.Key.EndsWith(".umap", StringComparison.OrdinalIgnoreCase) && (kv.Key.StartsWith(CellFolder, StringComparison.OrdinalIgnoreCase) || kv.Key.StartsWith(MainLevel + ".", StringComparison.OrdinalIgnoreCase))).Select(kv => kv.Value).ToList();
Console.Error.WriteLine($"reading {levels.Count} levels");
foreach (var level in levels)
{
    foreach (var actor in provider.LoadPackage(level).GetExports())
    {
        string cls = actor.Class?.Name.ToString() ?? "";
        if (cls != "BP_POI_Actor_C" && !landmarkClasses.Contains(cls)) continue;
        if (Location(actor) is not { } at) continue;
        if (cls == "BP_POI_Actor_C")
        {
            if (!actor.TryGetValue(out FPackageIndex dataRef, "Point Of Interest Data") || dataRef.Load<UObject>() is not { } data) continue;
            var props = Json(data).GetProperty("Properties");
            pois.Add(new
            {
                id = data.Name,
                name = props.TryGetProperty("DisplayName", out var dn) ? Text(dn) : null,
                category = props.TryGetProperty("CategoryTag", out var tag) ? tag.GetProperty("TagName").GetString() : null,
                x = at.X, y = at.Y, z = at.Z,
            });
        }
        else
        {
            landmarks.Add(new { cls, x = at.X, y = at.Y, z = at.Z });
        }
    }
}

// --- names ------------------------------------------------------------------------------

var items = new SortedDictionary<string, object>();
foreach (var table in provider.Files.Where(kv => kv.Key.Contains("/Table_Items", StringComparison.OrdinalIgnoreCase) && kv.Key.EndsWith(".uasset")).Select(kv => kv.Value))
{
    foreach (var export in Json(provider.LoadPackage(table).GetExports()).EnumerateArray())
    {
        if (!export.TryGetProperty("Rows", out var rows)) continue;
        foreach (var row in rows.EnumerateObject())
        {
            var name = row.Value.TryGetProperty("LocalizedDisplayName", out var dn) ? Text(dn) : null;
            if (name is null) continue;
            string? Tag(string prop) => row.Value.TryGetProperty(prop, out var t) && t.ValueKind == JsonValueKind.Object && t.TryGetProperty("TagName", out var n) ? n.GetString() : null;
            items[row.Name] = new { name, type = row.Value.TryGetProperty("ItemType", out var it) ? it.GetString() : null, keyItem = Tag("KeyItemCategory"), rarity = Tag("RarityTag") };
        }
    }
}
var characters = strings.TryGetValue("game/characters", out var chars) ? chars : new();

File.WriteAllText(Path.Combine(outDir, "data.json"), JsonSerializer.Serialize(new { mapBounds, abyssBounds, pois, landmarks, items, characters }, new JsonSerializerOptions { WriteIndented = true }));
Console.Error.WriteLine($"{pois.Count} points of interest, {landmarks.Count} landmarks, {items.Count} items");
return 0;

// --- helpers ----------------------------------------------------------------------------

static Dictionary<string, string> ParseArgs(string[] args)
{
    var opts = new Dictionary<string, string>();
    for (var i = 0; i + 1 < args.Length; i += 2) opts[args[i].TrimStart('-')] = args[i + 1];
    return opts;
}

static JsonElement Json(object value) => JsonDocument.Parse(Newtonsoft.Json.JsonConvert.SerializeObject(value)).RootElement;

static IEnumerable<JsonElement> Descendants(JsonElement e)
{
    yield return e;
    var children = e.ValueKind switch
    {
        JsonValueKind.Object => e.EnumerateObject().Select(p => p.Value),
        JsonValueKind.Array => e.EnumerateArray(),
        _ => [],
    };
    foreach (var c in children)
        foreach (var d in Descendants(c)) yield return d;
}

// An actor's position: its root component's, when that isn't attached to another one.
static FVector? Location(UObject actor)
{
    if (!actor.TryGetValue(out FPackageIndex root, "RootComponent") || root.IsNull || root.Load<UObject>() is not { } component) return null;
    if (component.TryGetValue(out FPackageIndex parent, "AttachParent") && !parent.IsNull) return null;
    return component.GetOrDefault<FVector>("RelativeLocation");
}

// Every English string, by table name and id, newest release last so it wins.
Dictionary<string, Dictionary<int, string>> LoadStrings()
{
    var tables = new Dictionary<string, Dictionary<int, string>>();
    var bundles = provider.Files.Keys.Where(k => k.Contains("/Localized/enus/Text/Text_enus.uasset", StringComparison.OrdinalIgnoreCase)).OrderBy(k => k);
    foreach (var bundle in bundles)
    {
        foreach (var export in Json(provider.LoadPackage(bundle).GetExports()).EnumerateArray())
        {
            if (!export.TryGetProperty("Properties", out var props) || !props.TryGetProperty("StringTables", out var list)) continue;
            foreach (var table in list.EnumerateArray())
            {
                var entries = tables.TryGetValue(table.GetProperty("Key").GetString()!, out var e) ? e : tables[table.GetProperty("Key").GetString()!] = new();
                foreach (var entry in table.GetProperty("Value").GetProperty("Entries").EnumerateArray())
                    entries[int.Parse(entry.GetProperty("Key").GetString()!)] = entry.GetProperty("Value").GetProperty("DefaultText").GetString() ?? "";
            }
        }
    }
    return tables;
}

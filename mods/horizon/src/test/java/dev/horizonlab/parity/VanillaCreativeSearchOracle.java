package dev.horizonlab.parity;

import com.google.gson.*;
import java.nio.charset.StandardCharsets;
import java.nio.file.*;
import java.util.*;
import net.minecraft.client.multiplayer.SessionSearchTrees;
import net.minecraft.client.resources.language.ClientLanguage;
import net.minecraft.core.registries.BuiltInRegistries;
import net.minecraft.locale.Language;
import net.minecraft.network.chat.Component;
import net.minecraft.server.packs.PackLocationInfo;
import net.minecraft.server.packs.PackType;
import net.minecraft.server.packs.PathPackResources;
import net.minecraft.server.packs.repository.PackSource;
import net.minecraft.server.packs.resources.MultiPackResourceManager;
import net.minecraft.world.flag.FeatureFlags;
import net.minecraft.world.item.CreativeModeTab;
import net.minecraft.world.item.CreativeModeTabs;
import net.minecraft.world.item.ItemStack;

/**
 * Original oracle for the creative inventory (test_creative_parity.py): writes
 * the exported creative-inventory.json Horizon loads, then what the vanilla
 * client shows: every tab's display items (canonical stacks) and, for each
 * query, CreativeModeInventoryScreen.refreshSearchResults' items as indices
 * into the search tab, searched through the client's own SessionSearchTrees
 * (FullTextSearchTree over creative tooltip lines and item ids, IdSearchTree
 * over item tags after "#").
 *
 *   VanillaCreativeSearchOracle QUERIES.json EXPORT.json OUT.json RESOURCES [permissions]
 *
 * With "permissions", the screen's tabs are built as for an operator in creative
 * with the Operator Items Tab option on (CreativeModeInventoryScreen.hasPermissions).
 *
 * The queries are the file's list, then every search stack's item path and
 * tooltip lines and every item tag, each also as a prefix and with "#".
 * Then the same search trees are reloaded in French for the focused queries.
 */
public final class VanillaCreativeSearchOracle {
    public static void main(String[] args) throws Exception {
        ParityData.init();
        var lookup = ParityData.lookup;
        boolean hasPermissions = args.length > 4 && args[4].equals("permissions");
        Files.writeString(Path.of(args[1]),
            VanillaCreativeTabsExporter.export(FeatureFlags.DEFAULT_FLAGS, lookup).toString(),
            StandardCharsets.UTF_8);
        // CreativeModeInventoryScreen.tryRebuildTabContents, as the screen calls it.
        CreativeModeTabs.tryRebuildTabContents(FeatureFlags.DEFAULT_FLAGS, hasPermissions, lookup);
        List<ItemStack> searchItems = List.copyOf(CreativeModeTabs.searchTab().getDisplayItems());
        var trees = new SessionSearchTrees();
        trees.updateCreativeTooltips(lookup, searchItems);
        trees.updateCreativeTags(searchItems);

        var out = new JsonObject();
        var tabs = new JsonArray();
        for (CreativeModeTab tab : BuiltInRegistries.CREATIVE_MODE_TAB) {
            var o = new JsonObject();
            o.addProperty("id", BuiltInRegistries.CREATIVE_MODE_TAB.getKey(tab).toString());
            o.addProperty("type", tab.getType().name());
            o.addProperty("shown", tab.shouldDisplay());
            o.addProperty("title", tab.getDisplayName().getString());
            o.addProperty("icon", BuiltInRegistries.ITEM.getKey(tab.getIconItem().getItem()).toString());
            o.addProperty("row", tab.row().name());
            o.addProperty("column", tab.column());
            var items = new JsonArray();
            for (ItemStack stack : tab.getDisplayItems()) items.add(ParityData.canon(stack));
            o.add("items", items);
            tabs.add(o);
        }
        out.add("tabs", tabs);

        var querySets = JsonParser.parseString(Files.readString(Path.of(args[0]))).getAsJsonObject();
        var queries = new LinkedHashSet<String>();
        for (var q : querySets.getAsJsonArray("english")) queries.add(q.getAsString());
        var context = net.minecraft.world.item.Item.TooltipContext.of(lookup);
        var flag = net.minecraft.world.item.TooltipFlag.Default.NORMAL.asCreative();
        var tags = new TreeSet<String>();
        for (ItemStack stack : searchItems) {
            String path = BuiltInRegistries.ITEM.getKey(stack.getItem()).getPath();
            queries.add(path);
            queries.add(path.substring(0, (path.length() + 1) / 2));
            for (String line : VanillaCreativeTabsExporter.tooltipLines(stack, context, flag)) {
                queries.add(line);
                queries.add(line.substring(0, (line.length() + 1) / 2));
            }
            stack.tags().forEach(t -> tags.add(t.location().toString()));
        }
        for (String tag : tags) {
            queries.add("#" + tag);
            queries.add("#" + tag.substring(tag.indexOf(':') + 1));
        }
        var index = new IdentityHashMap<ItemStack, Integer>();
        for (int i = 0; i < searchItems.size(); i++) index.put(searchItems.get(i), i);
        out.add("searches", queryResults(trees, index, queries));

        var pack = new PathPackResources(new PackLocationInfo("vanilla", Component.literal("vanilla"),
            PackSource.BUILT_IN, Optional.empty()), Path.of(args[3]));
        try (var resources = new MultiPackResourceManager(PackType.CLIENT_RESOURCES, List.of(pack))) {
            Language.inject(ClientLanguage.loadFrom(resources, List.of("en_us", "fr_fr"), false));
            trees.rebuildAfterLanguageChange();
            var french = new ArrayList<String>();
            for (var q : querySets.getAsJsonArray("french")) french.add(q.getAsString());
            out.add("frenchSearches", queryResults(trees, index, french));
        }
        Files.writeString(Path.of(args[2]), out.toString(), StandardCharsets.UTF_8);
    }

    private static JsonArray queryResults(SessionSearchTrees trees, Map<ItemStack, Integer> index,
                                          Iterable<String> queries) {
        var results = new JsonArray();
        for (String query : queries) {
            var r = new JsonObject();
            r.addProperty("query", query);
            var found = new JsonArray();
            for (ItemStack stack : refreshSearchResults(trees, query)) found.add(index.get(stack));
            r.add("results", found);
            results.add(r);
        }
        return results;
    }

    /** CreativeModeInventoryScreen.refreshSearchResults' items for the search box's value. */
    static List<ItemStack> refreshSearchResults(SessionSearchTrees trees, String searchTerm) {
        if (searchTerm.isEmpty()) return List.copyOf(CreativeModeTabs.searchTab().getDisplayItems());
        if (searchTerm.startsWith("#")) return trees.creativeTagSearch().search(searchTerm.substring(1).toLowerCase(Locale.ROOT));
        return trees.creativeNameSearch().search(searchTerm.toLowerCase(Locale.ROOT));
    }
}

package dev.horizonlab.parity;

import com.google.gson.*;
import com.mojang.serialization.JsonOps;
import java.nio.file.*;
import java.util.*;
import net.minecraft.SharedConstants;
import net.minecraft.core.*;
import net.minecraft.core.component.DataComponents;
import net.minecraft.core.registries.*;
import net.minecraft.resources.*;
import net.minecraft.server.*;
import net.minecraft.server.packs.*;
import net.minecraft.server.packs.repository.*;
import net.minecraft.server.packs.resources.*;
import net.minecraft.tags.TagLoader;
import net.minecraft.util.RandomSource;
import net.minecraft.world.item.*;
import net.minecraft.world.item.alchemy.*;
import net.minecraft.world.item.enchantment.EnchantmentHelper;
import net.minecraft.world.item.trading.*;
import net.minecraft.world.level.storage.loot.*;
import net.minecraft.world.level.storage.loot.functions.SetEnchantmentsFunction;

/** Direct vanilla methods only: loads registries, never creates a server/world. */
public final class VanillaTradeComponentsOracle {
    public static void main(String[] args) throws Exception {
        SharedConstants.tryDetectVersion();
        Bootstrap.bootStrap();
        var repo = ServerPacksSource.createVanillaTrustedRepository();
        repo.reload(); repo.setSelected(List.of("vanilla"));
        try (var resources = new MultiPackResourceManager(PackType.SERVER_DATA, repo.openAllSelected())) {
            var layers = RegistryLayer.createRegistryAccess();
            var tags = TagLoader.loadTagsForExistingRegistries(resources, layers.getLayer(RegistryLayer.STATIC));
            var context = TagLoader.buildUpdatedLookups(layers.getAccessForLoading(RegistryLayer.WORLD), tags);
            var world = RegistryDataLoader.load(resources, context, RegistryDataLoader.WORLD_REGISTRIES, Runnable::run).join();
            tags.forEach(Registry.PendingTags::apply);
            var access = layers.replaceFrom(RegistryLayer.WORLD, world).compositeAccess();
            BuiltInRegistries.DATA_COMPONENT_INITIALIZERS.build(access).forEach(p -> p.apply());
            var ops = access.createSerializationContext(JsonOps.INSTANCE);
            var ctor = LootContext.class.getDeclaredConstructor(LootParams.class, RandomSource.class, HolderGetter.Provider.class);
            ctor.setAccessible(true);
            var output = new ArrayList<String>();
            for (var line : Files.readAllLines(Path.of(args[0]))) {
                var input = JsonParser.parseString(line).getAsJsonObject();
                var t = input.getAsJsonObject("trade");
                var random = RandomSource.create(input.get("seed").getAsLong());
                var loot = ctor.newInstance(null, random, access);
                var gives = t.getAsJsonObject("gives");
                var stack = new ItemStack(BuiltInRegistries.ITEM.getValue(Identifier.parse(gives.get("id").getAsString())), gives.has("count") ? gives.get("count").getAsInt() : 1);
                if (t.has("given_item_modifier")) {
                    var mods = t.get("given_item_modifier");
                    var list = mods.isJsonArray() ? mods.getAsJsonArray() : new JsonArray();
                    if (!mods.isJsonArray()) list.add(mods);
                    for (var m : list) stack = SetEnchantmentsFunction.MAP_CODEC.codec().parse(ops, m).getOrThrow().run(stack, loot);
                }
                var a = TradeCost.CODEC.parse(ops, t.get("wants")).getOrThrow().toItemCost(loot, 0);
                var b = t.has("additional_wants") ? TradeCost.CODEC.parse(ops, t.get("additional_wants")).getOrThrow().toItemCost(loot, 0) : null;
                var row = new JsonObject();
                JsonElement result = JsonNull.INSTANCE;
                if (a.count() > 0 && (b == null || b.count() > 0)) {
                    var r = new JsonObject();
                    r.addProperty("item", BuiltInRegistries.ITEM.getKey(stack.getItem()).toString());
                    r.addProperty("count", stack.getCount());
                    var ench = new TreeMap<String,Integer>();
                    EnchantmentHelper.getEnchantmentsForCrafting(stack).entrySet().forEach(e -> ench.put(e.getKey().getRegisteredName(), e.getIntValue()));
                    var levels = new JsonArray();
                    ench.forEach((id,level) -> { var pair = new JsonArray(); pair.add(id); pair.add(level); levels.add(pair); });
                    r.add("levels", levels); r.addProperty("a", a.count());
                    if (b == null) r.add("b", JsonNull.INSTANCE); else r.addProperty("b", b.count());
                    var payments = new JsonArray();
                    var offer = new MerchantOffer(a, Optional.ofNullable(b), stack, 4, 1, 0);
                    for (var kind : List.of("plain", "renamed", "wrong", "extra_potion")) {
                        var p = new ItemStack(Items.POTION, 2);
                        p.set(DataComponents.POTION_CONTENTS, new PotionContents(kind.equals("wrong") ? Potions.AWKWARD : Potions.WATER));
                        if (kind.equals("renamed")) p.set(DataComponents.CUSTOM_NAME, net.minecraft.network.chat.Component.literal("Bottle"));
                        if (kind.equals("extra_potion")) p.set(DataComponents.POTION_CONTENTS, new PotionContents(Optional.of(Potions.WATER), Optional.of(123), List.of(), Optional.empty()));
                        var pair = new JsonArray(); pair.add(offer.take(p, ItemStack.EMPTY)); pair.add(p.getCount()); payments.add(pair);
                    }
                    r.add("payments", payments); result = r;
                }
                row.add("result", result); row.addProperty("next", random.nextLong()); output.add(row.toString());
            }
            Files.write(Path.of(args[1]), output);
        }
    }
}

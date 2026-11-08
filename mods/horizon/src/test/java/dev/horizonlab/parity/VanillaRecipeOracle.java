package dev.horizonlab.parity;

import java.io.PrintWriter;
import java.nio.charset.StandardCharsets;
import java.nio.file.*;
import java.util.*;
import net.minecraft.core.registries.BuiltInRegistries;
import net.minecraft.world.flag.FeatureFlags;
import net.minecraft.world.item.ItemStack;
import net.minecraft.world.item.crafting.*;

/**
 * Original oracle: vanilla's RecipeManager over the data pack's recipes.
 * Tab-separated commands (stacks are SNBT of ItemStack.CODEC, "-" is empty):
 * craft N cells..., cook TYPE stack, stonecut stack, smith template base addition.
 * "dump" prints every crafting recipe's ingredient item lists for the generator.
 */
public final class VanillaRecipeOracle {
    static RecipeManager recipes;

    public static void main(String[] args) throws Exception {
        ParityData.init();
        recipes = new RecipeManager(ParityData.lookup);
        recipes.finalizeRecipeLoading(FeatureFlags.VANILLA_SET);
        if (args[0].equals("dump")) {
            dump(Path.of(args[1]));
            return;
        }
        try (var out = new PrintWriter(Files.newBufferedWriter(Path.of(args[1]), StandardCharsets.UTF_8));
             var lines = Files.lines(Path.of(args[0]))) {
            for (String line : (Iterable<String>) lines::iterator) {
                String result;
                try {
                    result = run(line.split("\t", -1));
                } catch (Exception e) {
                    result = "error";
                }
                out.println(result);
            }
        }
    }

    static String run(String[] f) throws Exception {
        switch (f[0]) {
            case "craft": {
                int n = Integer.parseInt(f[1]);
                List<ItemStack> items = new ArrayList<>();
                for (int i = 0; i < n * n; i++) items.add(ParityData.stack(f[2 + i]));
                var positioned = CraftingInput.ofPositioned(n, n, items);
                var input = positioned.input();
                if (input.isEmpty()) return "none";
                var found = recipes.getRecipeFor(RecipeType.CRAFTING, input, null);
                if (found.isEmpty()) return "none";
                var recipe = found.get().value();
                var result = recipe.assemble(input);
                var rest = recipe.getRemainingItems(input);
                StringJoiner joiner = new StringJoiner(";");
                for (int i = 0; i < rest.size(); i++) {
                    if (rest.get(i).isEmpty()) continue;
                    int at = (positioned.top() + i / input.width()) * n + positioned.left() + i % input.width();
                    joiner.add(at + "=" + ParityData.canon(rest.get(i)));
                }
                return found.get().id().identifier().getPath() + "\t" + ParityData.canon(result) + "\t" + joiner;
            }
            case "cook": {
                RecipeType<? extends AbstractCookingRecipe> type = switch (f[1]) {
                    case "smelting" -> RecipeType.SMELTING;
                    case "blasting" -> RecipeType.BLASTING;
                    case "smoking" -> RecipeType.SMOKING;
                    default -> RecipeType.CAMPFIRE_COOKING;
                };
                return cook(type, ParityData.stack(f[2]));
            }
            case "stonecut": {
                var stack = ParityData.stack(f[1]);
                StringJoiner joiner = new StringJoiner(";");
                for (var entry : recipes.getSynchronizedStonecutterRecipes().selectByInput(stack).entries()) {
                    var holder = entry.recipe().recipe().get();
                    joiner.add(holder.id().identifier().getPath() + "=" + ParityData.canon(holder.value().assemble(new SingleRecipeInput(stack))));
                }
                return joiner.toString();
            }
            case "smith_transform": {
                var ops = net.minecraft.resources.RegistryOps.create(com.mojang.serialization.JsonOps.INSTANCE, ParityData.lookup);
                var decoded = SmithingTransformRecipe.MAP_CODEC.codec().parse(ops, com.google.gson.JsonParser.parseString(f[1])).result();
                if (decoded.isEmpty()) return "none";
                var recipe = decoded.get();
                var input = new SmithingRecipeInput(ParityData.stack(f[2]), ParityData.stack(f[3]), ParityData.stack(f[4]));
                if (!recipe.matches(input, ParityData.level)) return "none";
                var result = recipe.assemble(input);
                return result.isEmpty() ? "none" : ParityData.canon(result);
            }
            case "smith": {
                var input = new SmithingRecipeInput(ParityData.stack(f[1]), ParityData.stack(f[2]), ParityData.stack(f[3]));
                var found = recipes.getRecipeFor(RecipeType.SMITHING, input, null);
                if (found.isEmpty()) return "none";
                return ParityData.canon(found.get().value().assemble(input));
            }
            default:
                throw new IllegalArgumentException(f[0]);
        }
    }

    static <T extends AbstractCookingRecipe> String cook(RecipeType<T> type, ItemStack stack) {
        if (stack.isEmpty()) return "none";
        var input = new SingleRecipeInput(stack);
        var found = recipes.getRecipeFor(type, input, null);
        if (found.isEmpty()) return "none";
        var recipe = found.get().value();
        return found.get().id().identifier().getPath() + "\t" + ParityData.canon(recipe.assemble(input)) + "\t" + recipe.experience() + "\t" + recipe.cookingTime();
    }

    static String items(Ingredient ingredient) {
        if (ingredient == null) return "[]";
        StringBuilder row = new StringBuilder("[");
        var items = ingredient.items().toList();
        for (int i = 0; i < items.size(); i++) row.append(i > 0 ? "," : "").append('"').append(BuiltInRegistries.ITEM.getKey(items.get(i).value())).append('"');
        return row.append(']').toString();
    }

    static void dump(Path path) throws Exception {
        try (var out = new PrintWriter(Files.newBufferedWriter(path, StandardCharsets.UTF_8))) {
            for (var holder : recipes.getRecipes()) {
                var recipe = holder.value();
                if (recipe instanceof SmithingRecipe smithing) {
                    out.println("{\"id\":\"" + holder.id().identifier().getPath() + "\",\"kind\":\"" + recipe.getClass().getSimpleName()
                            + "\",\"template\":" + items(smithing.templateIngredient().orElse(null)) + ",\"base\":" + items(smithing.baseIngredient())
                            + ",\"addition\":" + items(smithing.additionIngredient().orElse(null)) + "}");
                    continue;
                }
                if (recipe.getType() != RecipeType.CRAFTING) continue;
                var info = recipe.placementInfo();
                StringBuilder row = new StringBuilder();
                row.append("{\"id\":\"").append(holder.id().identifier().getPath()).append("\",\"kind\":\"").append(recipe.getClass().getSimpleName()).append("\",\"slots\":[");
                for (int i = 0; i < info.slotsToIngredientIndex().size(); i++) row.append(i > 0 ? "," : "").append(info.slotsToIngredientIndex().getInt(i));
                row.append("],\"ingredients\":[");
                boolean firstIngredient = true;
                for (var ingredient : info.ingredients()) {
                    row.append(firstIngredient ? "" : ",").append('[');
                    firstIngredient = false;
                    var items = ingredient.items().toList();
                    for (int i = 0; i < items.size(); i++) row.append(i > 0 ? "," : "").append('"').append(BuiltInRegistries.ITEM.getKey(items.get(i).value())).append('"');
                    row.append(']');
                }
                row.append("]}");
                out.println(row);
            }
        }
    }
}

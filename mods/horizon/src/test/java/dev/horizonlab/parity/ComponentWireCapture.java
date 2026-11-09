package dev.horizonlab.parity;

import com.google.gson.JsonArray;
import com.google.gson.JsonObject;
import io.netty.buffer.Unpooled;
import java.util.HexFormat;
import net.minecraft.core.component.DataComponentExactPredicate;
import net.minecraft.core.component.DataComponentType;
import net.minecraft.core.component.predicates.DataComponentPredicate;
import net.minecraft.core.registries.BuiltInRegistries;
import net.minecraft.core.registries.Registries;
import net.minecraft.nbt.NbtOps;
import net.minecraft.nbt.Tag;
import net.minecraft.network.RegistryFriendlyByteBuf;
import net.minecraft.network.codec.ByteBufCodecs;
import net.minecraft.network.protocol.Packet;
import net.minecraft.network.protocol.game.*;
import net.minecraft.world.item.ItemStack;

/** Additional evidence from ORIGINAL packet bytes, after the normal full official decode. */
final class ComponentWireCapture {
    private static RegistryFriendlyByteBuf buffer(byte[] bytes) {
        return new RegistryFriendlyByteBuf(Unpooled.wrappedBuffer(bytes), ParityData.access);
    }

    private static String bytes(RegistryFriendlyByteBuf b, int start, int end) {
        byte[] value = new byte[end - start];
        b.getBytes(start, value);
        return HexFormat.of().formatHex(value);
    }

    private static JsonObject tag(Tag tag) {
        var result = new JsonObject();
        var b = new RegistryFriendlyByteBuf(Unpooled.buffer(), ParityData.access);
        ByteBufCodecs.TAG.encode(b, tag);
        result.addProperty("tag_type", tag.getId());
        result.addProperty("nbt", bytes(b, 0, b.writerIndex()));
        result.addProperty("snbt", tag.toString());
        return result;
    }

    @SuppressWarnings({"rawtypes", "unchecked"})
    private static JsonObject value(DataComponentType type, Object value) {
        return tag((Tag) type.codecOrThrow().encodeStart(
            ParityData.access.createSerializationContext(NbtOps.INSTANCE), value).getOrThrow());
    }

    /** Reads all fields using their official codecs; probes actual partial-value Tag types. */
    private static JsonArray predicates(byte[] original) {
        var b = buffer(original);
        var records = new JsonArray();
        int count = b.readVarInt();
        if (count < 0 || count > original.length) throw new IllegalStateException("predicate count");
        for (int i = 0; i < count; i++) {
            var row = new JsonObject();
            if (b.readBoolean()) ByteBufCodecs.holderSet(Registries.BLOCK).decode(b);
            if (b.readBoolean()) net.minecraft.advancements.predicates.StatePropertiesPredicate.STREAM_CODEC.decode(b);
            if (b.readBoolean()) row.add("block_nbt", tag(ByteBufCodecs.COMPOUND_TAG.decode(b)));
            DataComponentExactPredicate.STREAM_CODEC.decode(b);
            var partials = new JsonArray();
            int n = b.readVarInt();
            if (n < 0 || n > original.length) throw new IllegalStateException("partial count");
            for (int k = 0; k < n; k++) {
                int typeStart = b.readerIndex();
                var type = DataComponentPredicate.Type.STREAM_CODEC.decode(b);
                int valueStart = b.readerIndex();
                var typeBytes = buffer(HexFormat.of().parseHex(bytes(b, typeStart, valueStart)));
                boolean concrete = typeBytes.readBoolean();
                int id = typeBytes.readVarInt();
                if (typeBytes.isReadable()) throw new IllegalStateException("predicate type trailing bytes");
                var partial = new JsonObject();
                partial.addProperty("predicate_registry", concrete);
                partial.addProperty("type_id", id);
                // The actual official value codec independently consumes this original value.
                var checked = new RegistryFriendlyByteBuf(b.duplicate(), ParityData.access);
                type.singleStreamCodec().decode(checked);
                if (concrete) {
                    var actual = ByteBufCodecs.TAG.decode(b);
                    partial.add("wire_tag", tag(actual));
                    if (b.readerIndex() != checked.readerIndex()) throw new IllegalStateException("partial value boundary mismatch");
                } else {
                    b.readerIndex(checked.readerIndex());
                }
                partial.addProperty("raw", bytes(b, valueStart, b.readerIndex()));
                partials.add(partial);
            }
            row.add("partials", partials);
            records.add(row);
        }
        if (b.isReadable()) throw new IllegalStateException("adventure component trailing bytes: " + b.readableBytes());
        return records;
    }

    private static JsonObject stack(RegistryFriendlyByteBuf b, ItemStack decoded) {
        var row = new JsonObject();
        int start = b.readerIndex();
        int count = b.readVarInt();
        row.addProperty("count", count);
        if (count == 0) {
            if (!decoded.isEmpty()) throw new IllegalStateException("empty stack mismatch");
            return row;
        }
        int item = b.readVarInt();
        if (count != decoded.getCount() || item != BuiltInRegistries.ITEM.getId(decoded.getItem()))
            throw new IllegalStateException("official enclosing stack mismatch");
        row.addProperty("item_id", item);
        row.addProperty("item", BuiltInRegistries.ITEM.getKey(decoded.getItem()).toString());
        int added = b.readVarInt(), removed = b.readVarInt();
        if (added < 0 || removed < 0 || added + removed > b.readableBytes()) throw new IllegalStateException("patch counts");
        var additions = new JsonArray();
        for (int i = 0; i < added; i++) {
            int id = b.readVarInt();
            var type = BuiltInRegistries.DATA_COMPONENT_TYPE.byId(id);
            if (type == null) throw new IllegalStateException("unknown component " + id);
            int at = b.readerIndex();
            Object actual = type.streamCodec().decode(b);
            int end = b.readerIndex();
            var c = new JsonObject();
            String name = BuiltInRegistries.DATA_COMPONENT_TYPE.getKey(type).toString();
            c.addProperty("id", id);
            c.addProperty("name", name);
            c.addProperty("raw", bytes(b, at, end));
            c.add("value", value(type, actual));
            if (name.equals("minecraft:can_break") || name.equals("minecraft:can_place_on"))
                c.add("predicate_wire_tags", predicates(HexFormat.of().parseHex(bytes(b, at, end))));
            additions.add(c);
        }
        var removals = new JsonArray();
        for (int i = 0; i < removed; i++) {
            int id = b.readVarInt();
            if (BuiltInRegistries.DATA_COMPONENT_TYPE.byId(id) == null) throw new IllegalStateException("unknown removed component");
            removals.add(id);
        }
        row.add("added", additions);
        row.add("removed", removals);
        row.addProperty("raw", bytes(b, start, b.readerIndex()));
        // Effective values are independent of patch presence: prototypes and removals matter.
        var effective = new JsonObject();
        for (var c : decoded.getComponents()) {
            String name = BuiltInRegistries.DATA_COMPONENT_TYPE.getKey(c.type()).toString();
            if (name.equals("minecraft:provides_trim_material") || name.equals("minecraft:provides_banner_patterns")
                || name.equals("minecraft:can_break") || name.equals("minecraft:can_place_on"))
                effective.add(name, value(c.type(), c.value()));
        }
        row.add("effective", effective);
        return row;
    }

    static JsonObject inspect(Packet<?> packet, byte[] original) {
        if (!(packet instanceof ClientboundContainerSetSlotPacket || packet instanceof ClientboundContainerSetContentPacket
            || packet instanceof ClientboundSetPlayerInventoryPacket || packet instanceof ClientboundSetCursorItemPacket)) return null;
        var b = buffer(original);
        var row = new JsonObject();
        row.addProperty("packet_id", b.readVarInt());
        row.addProperty("packet", packet.type().id().toString());
        row.addProperty("wire", HexFormat.of().formatHex(original));
        var stacks = new JsonArray();
        if (packet instanceof ClientboundContainerSetSlotPacket p) {
            row.addProperty("kind", "slot");
            int window = b.readContainerId(), state = b.readVarInt(), slot = b.readShort();
            if (window != p.getContainerId() || state != p.getStateId() || slot != p.getSlot()) throw new IllegalStateException("slot header mismatch");
            row.addProperty("window", window); row.addProperty("state", state); row.addProperty("slot", slot);
            stacks.add(stack(b, p.getItem()));
        } else if (packet instanceof ClientboundContainerSetContentPacket p) {
            row.addProperty("kind", "content");
            int window = b.readContainerId(), state = b.readVarInt(), n = b.readVarInt();
            if (window != p.containerId() || state != p.stateId() || n != p.items().size()) throw new IllegalStateException("content header mismatch");
            row.addProperty("window", window); row.addProperty("state", state);
            for (var s : p.items()) stacks.add(stack(b, s));
            row.add("cursor", stack(b, p.carriedItem()));
        } else if (packet instanceof ClientboundSetPlayerInventoryPacket p) {
            row.addProperty("kind", "inventory");
            int slot = b.readVarInt();
            if (slot != p.slot()) throw new IllegalStateException("inventory header mismatch");
            row.addProperty("slot", slot); stacks.add(stack(b, p.contents()));
        } else if (packet instanceof ClientboundSetCursorItemPacket p) {
            row.addProperty("kind", "cursor"); stacks.add(stack(b, p.contents()));
        }
        if (b.isReadable()) throw new IllegalStateException("component observer enclosing payload trailing bytes: " + b.readableBytes());
        row.add("stacks", stacks); row.addProperty("remaining", b.readableBytes());
        return row;
    }
}

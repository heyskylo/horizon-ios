package dev.horizonlab.parity;

import com.google.gson.JsonArray;
import com.google.gson.JsonNull;
import com.google.gson.JsonParser;
import java.net.InetAddress;
import java.net.InetSocketAddress;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.Optional;
import net.minecraft.client.multiplayer.resolver.AddressCheck;
import net.minecraft.client.multiplayer.resolver.ResolvedServerAddress;
import net.minecraft.client.multiplayer.resolver.ServerAddress;
import net.minecraft.client.multiplayer.resolver.ServerAddressResolver;
import net.minecraft.client.multiplayer.resolver.ServerNameResolver;
import net.minecraft.client.multiplayer.resolver.ServerRedirectHandler;

/** ConnectScreen.startConnecting uses the resolved socket's host and port for login. */
public final class VanillaServerAddressOracle {
    public static void main(String[] args) throws Exception {
        if (args.length == 2) {
            JsonArray output = new JsonArray();
            try (var reader = Files.newBufferedReader(Path.of(args[0]))) {
                for (var input : JsonParser.parseReader(reader).getAsJsonArray()) {
                    // Preserve Horizon's existing outer ASCII whitespace trim;
                    // this corpus compares host/port syntax, not IDN mapping.
                    String text = input.getAsString().trim();
                    if (!ServerAddress.isValidAddress(text)) {
                        output.add(JsonNull.INSTANCE);
                    } else {
                        var address = ServerAddress.parseString(text);
                        JsonArray parsed = new JsonArray();
                        parsed.add(address.getHost());
                        parsed.add(address.getPort());
                        output.add(parsed);
                    }
                }
            }
            Files.writeString(Path.of(args[1]), output.toString());
            return;
        }
        AddressCheck allow = new AddressCheck() {
            public boolean isAllowed(ResolvedServerAddress address) { return true; }
            public boolean isAllowed(ServerAddress address) { return true; }
        };
        ServerAddressResolver addresses = address -> {
            try {
                return Optional.of(ResolvedServerAddress.from(new InetSocketAddress(
                    InetAddress.getByAddress(address.getHost(), new byte[]{127, 0, 0, 1}),
                    address.getPort())));
            } catch (Exception error) {
                throw new RuntimeException(error);
            }
        };
        var constructor = ServerNameResolver.class.getDeclaredConstructor(
            ServerAddressResolver.class, ServerRedirectHandler.class, AddressCheck.class);
        constructor.setAccessible(true);
        for (String scenario : new String[]{"direct", "redirect", "explicit"}) {
            int port = scenario.equals("explicit") ? 25566 : 25565;
            ServerRedirectHandler redirect = address ->
                scenario.equals("redirect") && address.getPort() == 25565
                    ? Optional.of(new ServerAddress("backend.example.org.", 25570))
                    : Optional.empty();
            var resolved = constructor.newInstance(addresses, redirect, allow)
                .resolveAddress(new ServerAddress("play.example.org", port))
                .orElseThrow().asInetSocketAddress();
            System.out.println("server_address=" + scenario + ":" + resolved.getHostName()
                + ":" + resolved.getPort());
        }
    }
}

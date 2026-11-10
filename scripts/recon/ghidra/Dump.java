// Decompile functions by address or by referenced string.
// Args: tokens. "0x…" = function containing that address; "callers:0x…" = its callers;
// "range:0xA-0xB" = every function starting in [A, B);
// anything else = every function referencing a defined string that contains the token.
// Output goes to the file named by the WP2_OUT environment variable.
// @category WP2
import ghidra.app.decompiler.DecompInterface;
import ghidra.app.decompiler.DecompileResults;
import ghidra.app.script.GhidraScript;
import ghidra.program.model.address.Address;
import ghidra.program.model.listing.*;
import ghidra.program.model.symbol.Reference;
import java.io.*;
import java.util.*;

public class Dump extends GhidraScript {
    private DecompInterface ifc;
    private PrintWriter out;
    private final Set<Long> done = new HashSet<>();

    @Override
    public void run() throws Exception {
        out = new PrintWriter(new FileWriter(System.getenv("WP2_OUT")));
        ifc = new DecompInterface();
        ifc.openProgram(currentProgram);
        for (String token : getScriptArgs()) {
            if (token.startsWith("0x")) {
                Address a = toAddr(Long.decode(token));
                if (getFunctionContaining(a) == null) {
                    disassemble(a);
                    createFunction(a, null);
                }
                emit(getFunctionContaining(a), token);
            } else if (token.startsWith("range:")) {
                String[] ab = token.substring(6).split("-");
                FunctionIterator fi = currentProgram.getFunctionManager().getFunctions(toAddr(Long.decode(ab[0])), true);
                long end = Long.decode(ab[1]);
                while (fi.hasNext()) {
                    Function f = fi.next();
                    if (f.getEntryPoint().getOffset() >= end) break;
                    emit(f, token);
                }
            } else if (token.startsWith("callers:")) {
                Function fn = getFunctionContaining(toAddr(Long.decode(token.substring(8))));
                if (fn == null) continue;
                for (Function c : fn.getCallingFunctions(monitor)) emit(c, "caller of " + fn.getName());
            } else {
                byString(token);
            }
        }
        out.close();
    }

    private void byString(String token) throws Exception {
        byte[] needle = token.getBytes("UTF-8");
        Address hit = currentProgram.getMinAddress();
        while ((hit = find(hit, needle)) != null) {
            Address start = hit;
            while (getByte(start.subtract(1)) != 0) start = start.subtract(1);
            for (Reference r : getReferencesTo(start)) {
                emit(getFunctionContaining(r.getFromAddress()), "str @" + start + " \"" + token + "\"");
            }
            byte[] le = new byte[4];
            long v = start.getOffset();
            for (int i = 0; i < 4; i++) le[i] = (byte) (v >> (8 * i));
            Address lit = currentProgram.getMinAddress();
            while ((lit = find(lit, le)) != null) {
                Function owner = getFunctionContaining(lit);
                emit(owner != null ? owner : getFunctionBefore(lit), "lit @" + lit + " -> \"" + token + "\"");
                for (Reference r : getReferencesTo(lit)) emit(getFunctionContaining(r.getFromAddress()), "pool ref \"" + token + "\"");
                lit = lit.add(1);
            }
            hit = hit.add(1);
        }
    }

    private void emit(Function fn, String why) {
        if (fn == null || !done.add(fn.getEntryPoint().getOffset())) return;
        DecompileResults res = ifc.decompileFunction(fn, 120, monitor);
        out.printf("// ===== %s @ %s (%s)%n", fn.getName(), fn.getEntryPoint(), why);
        out.println(res != null && res.decompileCompleted() ? res.getDecompiledFunction().getC() : "// decompile failed");
    }
}

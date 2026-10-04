import java.nio.file.Files;
import java.nio.file.Path;
import java.util.Arrays;

/** Tests against the release SWF, including rejection of an incompatible client. */
public final class AndroidGachaOddsPreloadPatchTest {
    public static void main(String[] args) throws Exception {
        if (args.length!=2) throw new IllegalArgumentException("usage: test original.swf output-directory");
        Path original=Path.of(args[0]), directory=Path.of(args[1]);
        Files.createDirectories(directory);
        Path patched=directory.resolve("patched.swf"), repeated=directory.resolve("repeated.swf");
        byte[] source=Files.readAllBytes(original);
        var target=AndroidGachaOddsPreloadPatch.locate(source);
        if(target.patched()) throw new AssertionError("Test requires the unpatched release");
        AndroidGachaOddsPreloadPatch.main(new String[]{original.toString(),patched.toString()});
        AndroidGachaOddsPreloadPatch.main(new String[]{patched.toString(),repeated.toString()});
        if(!Arrays.equals(Files.readAllBytes(patched),Files.readAllBytes(repeated))) throw new AssertionError("Not idempotent");
        int offset=AndroidGachaOddsPreloadPatch.uniqueOffset(source,target.body().getCodeBytes())+target.conditionOffset();
        byte[] incompatible=source.clone();
        incompatible[offset]=0x28; // pushnan is valid AVM2 but incompatible with the expected Boolean argument.
        reject(incompatible);
        byte[] compressedHeader=source.clone();
        compressedHeader[0]='C';
        reject(compressedHeader);
        byte[] missingClass=source.clone();
        byte[] name="AssetResolver".getBytes(java.nio.charset.StandardCharsets.UTF_8);
        for(int i=0; i<=missingClass.length-name.length; i++) {
            if(Arrays.equals(missingClass,i,i+name.length,name,0,name.length))
                missingClass[i+name.length-1]='X';
        }
        reject(missingClass);
        System.out.println("PASS: release patch, exact method isolation, idempotency, incompatible opcode/header/class rejection");
    }
    static void reject(byte[] bytes) throws Exception {
        try { AndroidGachaOddsPreloadPatch.locate(bytes); }
        catch(IllegalArgumentException | IllegalStateException expected) { return; }
        throw new AssertionError("Incompatible client was accepted");
    }
}

// Fix the CN Android result scene's missing equipment-odds preload.
// The common asset group must use the same unrestricted preload as the odds page.
// Locate the instruction semantically, then change exactly one byte in the FWS.
import com.jpexs.decompiler.flash.SWF;
import com.jpexs.decompiler.flash.abc.ABC;
import com.jpexs.decompiler.flash.abc.avm2.instructions.AVM2Instruction;
import com.jpexs.decompiler.flash.abc.types.MethodBody;
import com.jpexs.decompiler.flash.tags.ABCContainerTag;
import java.io.ByteArrayInputStream;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.ArrayList;
import java.util.Arrays;
import java.util.List;
import java.util.Set;

public final class AndroidGachaOddsPreloadPatch {
    record Target(ABC abc, int classIndex, MethodBody body, int conditionOffset, boolean patched) {}

    static Target locate(byte[] source) throws Exception {
        if (source.length < 8 || source[0]!='F' || source[1]!='W' || source[2]!='S')
            throw new IllegalArgumentException("Expected uncompressed FWS input");
        SWF swf = new SWF(new ByteArrayInputStream(source), false);
        List<Target> targets = new ArrayList<>();
        for (ABCContainerTag tag : swf.getAbcList()) {
            ABC abc = tag.getABC();
            int ci = abc.findClassByName("pinball.asset.AssetResolver");
            if (ci < 0) continue;
            MethodBody body = AbcMethodDigest.findStaticMethod(abc,ci,"getBuilder");
            List<AVM2Instruction> code = body.getCode().code;
            List<AVM2Instruction> arguments = new ArrayList<>();
            for (int i=1; i<code.size(); i++) {
                AVM2Instruction ins=code.get(i);
                if (!ins.definition.instructionName.equals("callpropvoid") || ins.operands.length!=2 || ins.operands[1]!=5) continue;
                String name=abc.constants.getString(abc.constants.getMultiname(ins.operands[0]).name_index);
                if (name.equals("resolveAssetPathCollection")) arguments.add(code.get(i-1));
            }
            if (arguments.size()!=2 || !arguments.get(1).definition.instructionName.equals("pushfalse"))
                throw new IllegalStateException("Unexpected gacha preload call structure");
            String first=arguments.get(0).definition.instructionName;
            if (!first.equals("pushtrue") && !first.equals("pushfalse"))
                throw new IllegalStateException("Unexpected common gacha preload argument");
            targets.add(new Target(abc,ci,body,Math.toIntExact(arguments.get(0).getAddress()),first.equals("pushfalse")));
        }
        if (targets.size()!=1) throw new IllegalStateException("Expected one AssetResolver class");
        return targets.get(0);
    }

    static int uniqueOffset(byte[] bytes, byte[] needle) {
        int result=-1;
        for (int i=0; i<=bytes.length-needle.length; i++) {
            if (bytes[i]!=needle[0]) continue;
            if (!Arrays.equals(bytes,i,i+needle.length,needle,0,needle.length)) continue;
            if (result>=0) throw new IllegalStateException("Method bytecode is not unique");
            result=i;
        }
        if (result<0) throw new IllegalStateException("Method bytecode absent from FWS");
        return result;
    }

    public static void main(String[] args) throws Exception {
        if (args.length!=2) throw new IllegalArgumentException("usage: AndroidGachaOddsPreloadPatch input.swf output.swf");
        Path input=Path.of(args[0]), output=Path.of(args[1]);
        if (input.toAbsolutePath().normalize().equals(output.toAbsolutePath().normalize()))
            throw new IllegalArgumentException("Input and output must differ");
        byte[] source=Files.readAllBytes(input);
        Target before=locate(source);
        int offset=uniqueOffset(source,before.body.getCodeBytes())+before.conditionOffset;
        byte[] patched=source.clone();
        if (source[offset]!=(before.patched?0x27:0x26)) throw new IllegalStateException("Opcode mismatch");
        patched[offset]=0x27;
        Target after=locate(patched);
        if (!after.patched) throw new IllegalStateException("Preload still restricted");
        var changed=AbcMethodDigest.changedMethods(
            AbcMethodDigest.digestStaticMethods(before.abc,before.classIndex),
            AbcMethodDigest.digestStaticMethods(after.abc,after.classIndex));
        if (!changed.equals(before.patched ? Set.of() : Set.of("getBuilder")))
            throw new IllegalStateException("Unexpected changed methods: "+changed);
        AbcMethodDigest.requireSameMethods(
            AbcMethodDigest.digestInstanceMethods(before.abc,before.classIndex),
            AbcMethodDigest.digestInstanceMethods(after.abc,after.classIndex),"Instance methods changed");
        int differences=0;
        for (int i=0;i<source.length;i++) if(source[i]!=patched[i]) differences++;
        if (differences!=(before.patched?0:1)) throw new IllegalStateException("Unexpected byte changes");
        Files.write(output,patched);
        System.out.printf("{\"byteOffset\":%d,\"changedBytes\":%d,\"bytes\":%d,\"alreadyPatched\":%s}%n",offset,differences,patched.length,before.patched);
    }
}

import java.lang.reflect.*;import java.nio.file.*;import java.util.*;
/** Regenerates block intervals and verified aliases from Java 25 public APIs.
 * Optional arg: Unicode 16 UCD Blocks.txt canonical display names, from
 * https://www.unicode.org/Public/16.0.0/ucd/Blocks.txt . Every candidate is
 * verified with Character.UnicodeBlock.forName, never blindly normalized. */
public class JavaBlockRanges {
 static final TreeMap<String,Character.UnicodeBlock> aliases=new TreeMap<>();
 static void candidate(String name){try{var b=Character.UnicodeBlock.forName(name);aliases.put(name.toUpperCase(Locale.ROOT),b);}catch(IllegalArgumentException ignored){}}
 static void names(String s){candidate(s);candidate(s.replace('_',' '));candidate(s.replace("_", ""));candidate(s.replace(" ",""));candidate(s.replace('-','_').replace(' ','_'));}
 public static void main(String[] args)throws Exception{
  var blocks=new TreeMap<String,Character.UnicodeBlock>();
  for(var f:Character.UnicodeBlock.class.getFields())if(f.getType()==Character.UnicodeBlock.class){var b=(Character.UnicodeBlock)f.get(null);blocks.put(b.toString(),b);names(f.getName());names(b.toString());}
  if(args.length>0)for(String line:Files.readAllLines(Path.of(args[0]))){line=line.split("#",2)[0];if(line.contains(";"))names(line.split(";",2)[1].trim());}
  var ids=new IdentityHashMap<Character.UnicodeBlock,Integer>();int next=0;for(var b:blocks.values())ids.put(b,next++);
  System.out.println("// Generated Java 25 UnicodeBlock.of / forName observations (Unicode 16).\n// Regenerate with tools/regex_reference/JavaBlockRanges.java and UCD 16 Blocks.txt.\n");
  System.out.println("#[rustfmt::skip]\nstatic INTERVALS: &[(u32,u32,u16)] = &[");
  int start=0;var old=Character.UnicodeBlock.of(0);for(int cp=1;cp<=0x110000;cp++){var b=cp==0x110000?null:Character.UnicodeBlock.of(cp);if(old!=b){if(old!=null)System.out.printf("    (0x%X, 0x%X, %d),%n",start,cp-1,ids.get(old));start=cp;old=b;}}
  System.out.println("];\n#[rustfmt::skip]\nstatic NAMES: &[(&str,u16)] = &[");for(var e:aliases.entrySet())System.out.printf("    (\"%s\", %d),%n",e.getKey(),ids.get(e.getValue()));System.out.println("];\npub fn resolve(name: &str) -> Option<u16> { let key=name.to_uppercase(); NAMES.binary_search_by_key(&key.as_str(), |&(n,_)| n).ok().map(|i| NAMES[i].1) }\npub fn contains(block:u16,ch:char)->bool { let cp=ch as u32; let i=INTERVALS.partition_point(|&(lo,_,_)| lo<=cp); i>0 && cp<=INTERVALS[i-1].1 && INTERVALS[i-1].2==block }");
 }
}

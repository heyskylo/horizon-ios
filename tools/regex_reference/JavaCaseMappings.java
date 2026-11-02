/** Regenerates simple Unicode case data from the pinned Java 25 reference (Unicode 16). */
public class JavaCaseMappings {
 public static void main(String[]args) {
  System.out.println("// Generated from Java 25 Character case-mapping results (Unicode 16).\n// Regenerate with tools/regex_reference/JavaCaseMappings.java.\n");
  for(boolean upper:new boolean[]{true,false}) {
   System.out.println("#[rustfmt::skip]\nstatic "+(upper?"UPPER":"LOWER")+": &[(u32, u32)] = &[");
   for(int cp=0;cp<=0x10ffff;cp++){int mapped=upper?Character.toUpperCase(cp):Character.toLowerCase(cp);if(mapped!=cp)System.out.printf("    (0x%X, 0x%X),%n",cp,mapped);}
   System.out.println("];");
  }
  System.out.println("pub fn upper(c: char) -> char { mapped(UPPER, c) }\npub fn lower(c: char) -> char { mapped(LOWER, c) }\nfn mapped(table: &[(u32,u32)], c: char) -> char { table.binary_search_by_key(&(c as u32), |&(a,_)| a).ok().and_then(|i| char::from_u32(table[i].1)).unwrap_or(c) }");
 }
}

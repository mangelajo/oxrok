# Licensing

Not legal advice. The policy oxrok follows:

- Code written for oxrok from its own documents (protocol notes, specs,
  public interface facts) is MIT OR Apache-2.0.
- Code ported from libsigrok, libsigrokdecode or a sigrok decoder is a
  derived work and keeps its license (GPL-2.0-or-later or
  GPL-3.0-or-later). It lives in a separate crate with its own LICENSE
  file, behind a cargo feature, so a permissive build exists and a full
  build is GPL as a whole.
- Firmware blobs loaded onto a device are separate programs. Shipping
  them next to the binary is aggregation. Proprietary firmware is never
  shipped; the user installs it from the vendor.
- Python decoders from the sigrok distribution are loaded at run time
  from the user's install or a decoders directory. A package that bundles
  them is GPL as a whole.
- "sigrok" is the name of another project. oxrok describes itself as
  sigrok compatible and never uses the name as its own.

use std::{path::PathBuf,process::Command};
fn main(){
 println!("cargo:rerun-if-changed=assets/icon.ico");
 if std::env::var("CARGO_CFG_TARGET_OS").as_deref()!=Ok("windows"){return}
 let out=PathBuf::from(std::env::var_os("OUT_DIR").unwrap());let root=PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR").unwrap());
 let rc=out.join("icon.rc");let res=out.join("icon.res");
 std::fs::write(&rc,format!("1 ICON \"{}\"\n",root.join("assets/icon.ico").display().to_string().replace('\\',"/"))).unwrap();
 let mut candidates=vec![PathBuf::from("rc.exe")];
 if let Some(p)=std::env::var_os("ProgramFiles(x86)"){
  let bin=PathBuf::from(p).join("Windows Kits/10/bin");
  if let Ok(entries)=std::fs::read_dir(bin){let mut dirs:Vec<_>=entries.flatten().map(|e|e.path()).collect();dirs.sort();dirs.reverse();for dir in dirs{candidates.push(dir.join("x64/rc.exe"));}}
 }
 for exe in candidates{if let Ok(status)=Command::new(exe).arg("/nologo").arg("/fo").arg(&res).arg(&rc).status(){if status.success(){println!("cargo:rustc-link-arg-bins={}",res.display());return}}}
 panic!("Windows SDK Resource Compiler (rc.exe) is required to embed the application icon");
}

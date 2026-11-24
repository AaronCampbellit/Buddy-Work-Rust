// node package.mjs <new-output-directory> <pinned-camera-directory> [compiled-BuddyRust.exe]
import {spawnSync} from "node:child_process";
import {createHash} from "node:crypto";
import {access, copyFile, cp, mkdir, mkdtemp, readFile, readdir, rm, stat, writeFile} from "node:fs/promises";
import path from "node:path";
import {fileURLToPath} from "node:url";

const here = fileURLToPath(new URL(".",import.meta.url));
const repo = path.dirname(path.resolve(here));
const [, , outputArg, cameraArg, binaryArg] = process.argv;
if (process.platform !== "win32" || !outputArg || !cameraArg) {
  throw Error("Supply a fresh Windows output directory and the pinned camera payload.");
}
const output = path.resolve(outputArg);
const camera = path.resolve(cameraArg);
const binary = binaryArg ? path.resolve(binaryArg) : path.join(here,"target/release/BuddyRust.exe");
const app = path.join(output,"Buddy");
await access(binary);
function run(exe,args,capture = false,options = {}) {
  const result=spawnSync(exe,args,{cwd:here,windowsHide:true,stdio:capture?"pipe":"inherit",encoding:"utf8",maxBuffer:16*1024*1024,...options});
  if (result.error) throw result.error;
  if (result.status!==0) throw Error(path.basename(exe)+" exited "+result.status+" "+(result.stderr || ""));
  return result.stdout;
}
async function findTool(variable, parent, suffix) {
  if (process.env[variable]) return process.env[variable];
  const dirs = (await readdir(parent)).sort((a,b)=>b.localeCompare(a,undefined,{numeric:true}));
  for(const dir of dirs) {
    const exe=path.join(parent,dir,suffix);
    if(await stat(exe).then(s=>s.isFile(),()=>false)) return exe;
  }
  throw Error("Set "+variable+" to the installed tool.");
}
const nsis = await findTool("BUDDY_NSIS_PATH",path.join(process.env.LOCALAPPDATA,"electron-builder/Cache/nsis-3.0.4.1"),"makensis.exe");
const packer = await findTool("BUDDY_MAKEAPPX",path.join(process.env["ProgramFiles(x86)"],"Windows Kits/10/bin"),"x64/makeappx.exe");
const cargo = process.env.BUDDY_CARGO || path.join(process.env.USERPROFILE,".cargo/bin/cargo.exe");
const meta = JSON.parse(run(cargo,["metadata","--locked","--offline","--format-version","1","--filter-platform","x86_64-pc-windows-msvc"],true));
const pkg=meta.packages.find(p=>p.name==="buddy-work-native");
if(!pkg)throw Error("Unable to find Buddy in the crate metadata.");
const version=pkg.version;
const pin=JSON.parse(await readFile(path.join(repo,"camera-component.json"),"utf8"));
const cameraManifest=JSON.parse(await readFile(path.join(camera,"component-manifest.json"),"utf8"));
if(cameraManifest.repository!==pin.repository || cameraManifest.commit!==pin.commit) throw Error("Incorrect camera component.");
await mkdir(path.dirname(output),{recursive:true});
await mkdir(output);
const work=await mkdtemp(path.join(output,"pack-"));
try {
  await mkdir(app);
  await copyFile(binary,path.join(app,"BuddyRust.exe"));
  for (const relative of ["BuddyCameraHost.exe","BuddyCameraSource.dll","ai/manifest.json","ai/onnxruntime.dll","ai/onnxruntime_providers_shared.dll","ai/person-segmentation.onnx","ai/ONNX-RUNTIME-LICENSE.txt","ai/ONNX-RUNTIME-NOTICES.txt","ai/PERSON-MODEL-LICENSE.txt"]) {
    const buf=await readFile(path.join(camera,relative));
    if(createHash("sha256").update(buf).digest("hex")!==cameraManifest.hashes[relative]) throw Error("Camera verification failed for "+relative);
    const file=path.join(app,"camera",relative);
    await mkdir(path.dirname(file),{recursive:true});await writeFile(file,buf);
  }
  await copyFile(path.join(camera,"component-manifest.json"),path.join(app,"camera/component-manifest.json"));
  const models=path.join(app,"models");
  if(process.env.BUDDY_OCR_MODELS) {
    await mkdir(models);
    for(const name of ["text-detection.onnx","text-recognition.onnx"]) {
      const from=path.join(process.env.BUDDY_OCR_MODELS,name);
      if(await stat(from).then(s=>s.isFile(),()=>false)) await copyFile(from,path.join(models,name));
    }
  }
  run(process.execPath,[path.join(here,"devices-assets/stage-ocr.mjs"),models]);
  const licenses=path.join(app,"licenses");
  await mkdir(licenses);
  await copyFile(path.join(repo,"native-rust","lucide-LICENSE"),path.join(licenses,"lucide-LICENSE"));
  const index=[];
  for(const dep of meta.packages.filter(p=>p.id!==pkg.id).sort((a,b)=>a.id.localeCompare(b.id))) {
    const dir=path.dirname(dep.manifest_path);
    const entries=await readdir(dir,{withFileTypes:true});
    const docs=entries.filter(e=>e.isFile() && /^(licen[cs]e|copying|notice|copyright|unlicense)/i.test(e.name)).map(e=>e.name);
    if(dep.license_file && !docs.includes(dep.license_file)) docs.push(dep.license_file);
    const prefix=dep.name+"-"+dep.version;
    for(const doc of docs) await copyFile(path.resolve(dir,doc),path.join(licenses,prefix+"-"+path.basename(doc)));
    index.push([dep.name,dep.version,dep.license||"",dep.repository||"",...docs].join("\t"));
  }
  await writeFile(path.join(licenses,"crates.tsv"),index.join("\r\n"));
  const entries=await readdir(app,{recursive:true,withFileTypes:true});
  const relative=e=>path.relative(app,path.join(e.parentPath,e.name));
  const files=entries.filter(e=>e.isFile()).map(relative).sort();
  const dirs=entries.filter(e=>e.isDirectory()).map(relative).sort((a,b)=>b.length-a.length);
  if([...files,...dirs].some(p=>/["$\r\n]/.test(p))) throw Error("Unsupported character in a package file name.");
  const deleteList=path.join(work,"files.nsh");
  await writeFile(deleteList,[...files.map(f=>'Delete "$INSTDIR\\'+f+'"'),...dirs.map(d=>'RMDir "$INSTDIR\\'+d+'"')].join("\r\n"));
  run(nsis,["/V2","/DPAYLOAD="+app,"/DOUTPUT="+path.join(output,"Buddy-Work-Rust-Setup-x64.exe"),"/DDELETEFILES="+deleteList,"/DBUDDYVERSION="+version,path.join(here,"packaging/installer.nsi")]);
  run(path.join(process.env.WINDIR,"System32/tar.exe"),["-a","-cf",path.join(output,"Buddy-Work-Rust-Portable-x64.zip"),"Buddy"],false,{cwd:output});

  // The MSIX is an additional deployment format for capabilities that require package identity.
  const msixStage=path.join(work,"msix");
  await cp(app,msixStage,{recursive:true});
  const match=/^(\d+)\.(\d+)\.(\d+)(?:-beta\.(\d+))?$/.exec(version);
  if(!match) throw Error("Unsupported numeric MSIX version.");
  const numbers=[...match.slice(1,4).map(Number),Number(match[4]??65535)];
  if(numbers.some(n=>n>65535) || (match[4] && numbers[3]>=65535))throw Error("MSIX version component out of range.");
  const publisher=process.env.BUDDY_MSIX_PUBLISHER || "CN=Buddy Work Prototype";
  if(/[<>\u0000-\u001f]/.test(publisher)) throw Error("Invalid MSIX publisher name.");
  const manifest=(await readFile(path.join(here,"packaging/Package.appxmanifest"),"utf8"))
    .replace("@VERSION@",numbers.join(".")).replace("@PUBLISHER@",()=>publisher.replaceAll("&","&amp;").replaceAll('"',"&quot;"));
  await writeFile(path.join(msixStage,"AppxManifest.xml"),manifest);
  await mkdir(path.join(msixStage,"Assets"));
  const assets=[
    "$ErrorActionPreference='Stop'","Add-Type -AssemblyName System.Drawing",
    "$source=[Drawing.Image]::FromFile($env:BUDDY_ICON)",
    "try { foreach($entry in @(@('Store',50),@('Tile44',44),@('Tile150',150))) {",
    "$bitmap=New-Object Drawing.Bitmap($source,[int]$entry[1],[int]$entry[1])",
    "try { $bitmap.Save((Join-Path $env:BUDDY_ASSETS ($entry[0]+'.png')),[Drawing.Imaging.ImageFormat]::Png) } finally {$bitmap.Dispose()}",
    "} } finally {$source.Dispose()}",
  ].join("\n");
  run(path.join(process.env.WINDIR,"System32/WindowsPowerShell/v1.0/powershell.exe"),["-NoProfile","-NonInteractive","-Command",assets],false,
    {env:{...process.env,BUDDY_ICON:path.join(repo,"build/buddy-icon.png"),BUDDY_ASSETS:path.join(msixStage,"Assets")}});
  run(packer,["pack","/d",msixStage,"/p",path.join(output,"Buddy-Work-Rust-x64-unsigned.msix"),"/no"]);
} finally {
  await rm(work,{recursive:true,force:true});
}

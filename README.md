concat

a very simple tool to join together as much files as you would like,  
without using stdin stdout pipeline `>` `<` etc..  

multiple OS are supported.

<hr/>

the Windows amd64 build is `x86_64-pc-windows-msvc`  
it has two binaries. `concat.exe` and `concatw.exe`  
both are same expect that the `concatw.exe` has its own subsystem changed to Windows subsystem.  
it means the Windows OS will not assign a console window to the exe,  
meaning no black window when it runs. and since it uses arguments it can still work fine.  
you won't see any error messages though.  

<hr/>

get the latest binaries from the release section, same name convention.
https://github.com/eladkarako/concat/releases/latest  


Windows (amd64)  
- x86_64-pc-windows-msvc.zip - https://github.com/eladkarako/concat/releases/latest/download/x86_64-pc-windows-msvc.zip  

Android (built with NDK r27c)  
- aarch64-linux-android.zip - https://github.com/eladkarako/concat/releases/latest/download/aarch64-linux-android.zip  
- armv7-linux-androideabi.zip - https://github.com/eladkarako/concat/releases/latest/download/armv7-linux-androideabi.zip  
- x86_64-linux-android.zip - https://github.com/eladkarako/concat/releases/latest/download/x86_64-linux-android.zip  

Linux (amd64)  
- x86_64-unknown-linux-gnu.zip - https://github.com/eladkarako/concat/releases/latest/download/x86_64-unknown-linux-gnu.zip  
- x86_64-unknown-linux-musl.zip - https://github.com/eladkarako/concat/releases/latest/download/x86_64-unknown-linux-musl.zip  


<hr/>

optional argument to add some spacing between files (otherwise they are binary copy)  

`"--sep=SEPARATOR"` or `"--sep" "SEPARATOR"`  

you can use several preserved phrases to make the separator nicer, thouse will be replaced in-program:  
- `####r####` - `\r`
- `####n####` - `\n`
- `####t####` - `\t`
- `####file####` - `C:\123\456\foo.bar`
- `####name####` - `foo`
- `####ext####` - `bar` (or `.bar` not sure..)  
- `####index####` - 1 based index in entire list of files provided, 1 to `total`. will change every time matching the current file.
- `####total####` - 1 based index in entire list of files provided, 1 to `total`. will stay the same.

for example if you are concat. multiple `.reg` files (their name usually does not matter) you can add to make the editor process easier, note the `;` is making a line a comment line in `ini` and `reg` files.  
`--sep "####r########n########r########n####;;; - ####index#### / ####total#### ####r########n####"`  


<hr/>

<hr/>

<hr/>


<h1>notes, build.</h1>

although it it quick and dirty rust code.  
I figured it might be useful for anyone other than myself.  

Windows and Android were built on Windows 11 OS,  
Linux builds were built on WSL Ubunto.

it is darn easy to code and build in rust!  
this was intended to quickly cover few core issues, wsl for linux, android NDK for android, .. 

<hr/>

Windows
`rustup target add x86_64-pc-windows-msvc`  
`cargo build --release --target x86_64-pc-windows-msvc`  

<details><title>add manifest</title>  

```xml
<?xml version="1.0" encoding="UTF-8" standalone="yes"?> 
<assembly xmlns="urn:schemas-microsoft-com:asm.v1" manifestVersion="1.0"> 
  <dependency optional="yes"> 
    <dependentAssembly> 
      <assemblyIdentity name="Microsoft.Windows.Common-Controls" 
        version="6.0.0.0" publicKeyToken="6595b64144ccf1df" 
        type="win32" processorArchitecture="*" language="*" /> 
    </dependentAssembly> 
  </dependency> 
  <application xmlns="urn:schemas-microsoft-com:asm.v3"> 
     <windowsSettings> <dpiAware      xmlns="http://schemas.microsoft.com/SMI/2005/WindowsSettings">true/PM</dpiAware>                     </windowsSettings> 
     <windowsSettings> <dpiAwareness  xmlns="http://schemas.microsoft.com/SMI/2016/WindowsSettings">PerMonitorV2,PerMonitor</dpiAwareness> </windowsSettings> 
     <windowsSettings> <longPathAware xmlns="http://schemas.microsoft.com/SMI/2016/WindowsSettings">true</longPathAware>                   </windowsSettings> 
     <windowsSettings> <heapType      xmlns="http://schemas.microsoft.com/SMI/2020/WindowsSettings">SegmentHeap</heapType>                 </windowsSettings> 
  </application> 
  <trustInfo xmlns="urn:schemas-microsoft-com:asm.v3"> 
    <security> 
      <requestedPrivileges> 
        <requestedExecutionLevel level="asInvoker" uiAccess="false" /> 
      </requestedPrivileges> 
    </security> 
  </trustInfo> 
  <compatibility xmlns="urn:schemas-microsoft-com:compatibility.v1"> 
    <application> 
      <supportedOS Id="{8e0f7a12-bfb3-4fe8-b9a5-48fd50a15a9a}" /> 
      <supportedOS Id="{1f676c76-80e1-4239-95bb-83d0f6d0da78}" /> 
      <supportedOS Id="{4a2f28e3-53b9-4441-ba9c-d69d4a4a6e38}" /> 
      <supportedOS Id="{35138b9a-5d96-4fbd-8e2d-a2440225f93a}" /> 
      <supportedOS Id="{e2011457-1546-43c5-a5fe-008deee3d3f0}" /> 
    </application> 
  </compatibility> 
</assembly> 
 
```

</details>

locate `mt.exe` in your windows SDK.  
copy above manifest to a file named `app.manifest` and run:  
`mt.exe -nologo -manifest "app.manifest" -outputresource:concat.exe;1

copy exe and rename the copy `concatw.exe`.  
locate `editbin.exe` in Windows SDK or build tools,  
run `editbin.exe /NOLOGO "/SUBSYSTEM:WINDOWS" concatw.exe`  

optionally embed icon using reshacker instead of compiling rc to res.


Android (on Windows)  
install NDK, `C:\Program Files (x86)\Android\AndroidNDK\android-ndk-r27c`  
those commands are for PowerShell.

`rustup target add aarch64-linux-android`  
`rustup update`  
`$env:NDK='C:\Program Files (x86)\Android\AndroidNDK\android-ndk-r27c'`  
`$env:CC_aarch64_linux_android = "$env:NDK\toolchains\llvm\prebuilt\windows-x86_64\bin\aarch64-linux-android21-clang.cmd"`  
`$env:CARGO_TARGET_AARCH64_LINUX_ANDROID_LINKER = $env:CC_aarch64_linux_android`  
`cargo build --release --target aarch64-linux-android`  

`rustup target add armv7-linux-androideabi`  
`rustup update`  
`$env:CC_armv7_linux_androideabi = "$env:NDK\toolchains\llvm\prebuilt\windows-x86_64\bin\armv7a-linux-androideabi21-clang.cmd"`  
`$env:CARGO_TARGET_ARMV7_LINUX_ANDROIDEABI_LINKER = $env:CC_armv7_linux_androideabi`  
`cargo build --release --target armv7-linux-androideabi`  

`rustup target add x86_64-linux-android`  
`rustup update`  
`$env:CC_x86_64_linux_android = "$env:NDK\toolchains\llvm\prebuilt\windows-x86_64\bin\x86_64-linux-android21-clang.cmd"`  
`$env:CARGO_TARGET_X86_64_LINUX_ANDROID_LINKER = $env:CC_x86_64_linux_android`  
`cargo build --release --target x86_64-linux-android`  

<hr/>

Linux

download latest wsl msi, install the msi. beta is fine.  
https://github.com/microsoft/WSL/releases  

run as admin  
`dism.exe /online /enable-feature /featurename:VirtualMachinePlatform /all /norestart`  

(restart computer)  

`wsl --install`  
(when asked, enter user, password)

start `wsl.exe -d Ubuntu`  
very important to not work through `/mnt/c/foo/.......` for example - `cd ~`  
`sudo apt update && sudo apt install -y build-essential musl-tools pkg-config curl clang`  
just to make sure - `sudo apt remove rustup`  
`curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs -o rustup-init.sh`  
`sh rustup-init.sh`  
use default configuration.  

run after installation: `source $HOME/.cargo/env`  

from (rustrover) project folder `C:\Users\Elad\RustroverProjects\concat`  
open cmd or powershell in this windows folder,  
run:  
`wsl.exe -d Ubuntu -cd "%CD%"`  

`rustup target add x86_64-unknown-linux-gnu`  
`rustup target add x86_64-unknown-linux-musl`  
`rustup update`  
`cargo build --release --target x86_64-unknown-linux-gnu`  
`cargo build --release --target x86_64-unknown-linux-musl`  

<hr/>

the binaries are packed into zip to make them less of a security risk when downloaded.

note `x86_64-pc-windows-msvc.zip` has false positive `VHO:Trojan.Win64.Agent.gen` in Kaspersky, https://www.virustotal.com/gui/file/8949fc2ab40d384f136cd5841451e13d090ae0c85a1b63e535dcf9481c30baf3?nocache=1  

`x86_64-linux-android.zip`  
https://www.virustotal.com/gui/file/88648e1f565682e86cdf55125539609f4135040bf242bbf56405856fb3005121?nocache=1  

`armv7-linux-androideabi.zip`  
https://www.virustotal.com/gui/file/8c1b9fd2c87f3d9ea5be2297eefd7a4aa9ac86b526b8660354f64b457fe9ec3e?nocache=1  

`aarch64-linux-android.zip`  
https://www.virustotal.com/gui/file/3050a9d4c378e44212d83217067640e19cfbfc3ecf6ea114f234d8aa99a4d702?nocache=1  

`x86_64-unknown-linux-gnu.zip`  
https://www.virustotal.com/gui/file/f0ae51b16f2696f1567d7b264af7547a00f7109a597d6e1ea0190a0edc317f3b?nocache=1  

`x86_64-unknown-linux-musl.zip`  
https://www.virustotal.com/gui/file/f62693725552404a09c3d81c563fb4a2a63085520ec29935a14a0a8c0d77284f?nocache=1  

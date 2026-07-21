from PyInstaller.utils.hooks import collect_all

datas, binaries, hiddenimports = collect_all("mitmproxy")
hiddenimports += [
    "mitmproxy.addons.core",
    "mitmproxy.proxy.mode_servers",
    "mitmproxy_rs",
]

from PyInstaller.utils.hooks import collect_data_files

datas = collect_data_files("local_image_filter", includes=["resources/*.toml"])

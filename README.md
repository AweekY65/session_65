# Local Image Pyramid & Tile Generator

一个纯本地的图像金字塔与切片工具：所有输入图片、缩放层、tile、manifest
和缓存只存放在本地文件或内存中，**不依赖地图服务器、CDN、云存储或任何
外部服务**。

## 功能

- 读取常见 PNG / JPEG（经 Pillow 解码，统一转为 RGB）。
- 按配置逐级缩小生成 image pyramid，直到达到最小尺寸。
- 每级按固定 tile size 切片，边缘 tile 采用**裁剪**规则（见下文）。
- 支持 `nearest` 与 `bilinear` 两种缩放方式，实现基于 numpy 纯 float64
  运算，相同输入保证得到逐字节稳定的结果。
- 生成 `manifest.json`，记录原图尺寸、层级、tile 坐标、尺寸与 SHA-256 哈希。
- 增量重建：输入与配置未变化时不重复生成任何 tile；损坏或缺失的 tile
  会被自动重新生成。
- 所有写入经由临时文件 + `os.replace` 原子替换，manifest 最后写入，
  生成中断不会留下被 manifest 误认为有效的半文件。

## 层级算法

- Level 0 为原图，尺寸 `(W, H)`。
- 每一级宽高各减半（向上取整，最小为 1）：
  `w' = max(1, ceil(w/2))`，`h' = max(1, ceil(h/2))`。
- 当 `max(w, h) <= min_size`（默认 256）时停止，该级为最后一级。
- 例：`333x257`、`min_size=32` →
  `333x257 → 167x129 → 84x65 → 42x33 → 21x17`。

## 坐标与切片规则

- tile 尺寸固定为 `tile_size`（默认 256）。
- tile 坐标 `(tx, ty)` 从 0 开始，原点在图像左上角；
  像素偏移为 `(tx * tile_size, ty * tile_size)`。
- **边缘规则：裁剪（crop），不 padding。** 最右/最下列不足一个完整
  tile 时，tile 只保存真实内容，实际宽高记录在 manifest 的
  `width`/`height` 字段中。
- 存储路径：`tiles/<level>/<tx>/<ty>.png`，一律使用无损 PNG 编码。

## Manifest

`manifest.json` 结构：

```json
{
  "version": 1,
  "source": {"path": "...", "sha256": "...", "width": 769, "height": 513},
  "config": {"tile_size": 256, "min_size": 256, "resample": "bilinear"},
  "levels": [
    {
      "level": 0, "width": 769, "height": 513, "tiles_x": 4, "tiles_y": 3,
      "tiles": [
        {"x": 0, "y": 0, "width": 256, "height": 256,
         "path": "tiles/0/0/0.png", "sha256": "..."}
      ]
    }
  ]
}
```

## 增量重建与原子写

- 每次构建先计算源文件 SHA-256 并与 manifest 比对；源或配置变化时
  全部重建，否则逐 tile 校验「文件存在 + 哈希匹配」，只重新生成
  损坏、缺失或哈希不符的 tile。
- tile 与 manifest 均先写入同目录临时文件（`.tmp-*`）再 `os.replace`
  原子替换；manifest 最后写入。构建中断时 manifest 不会引用任何
  半文件，残留的 `.tmp-*` 会在下次运行时自动清理。

## 使用

```bash
# 构建
python -m pyramid.cli input.png -o out/ --tile-size 256 --min-size 256 --resample bilinear

# 校验已有输出（manifest + 全部 tile 哈希）
python -m pyramid.cli -o out/ --validate
```

Python API：

```python
from pyramid import PyramidConfig, build_pyramid, validate_output

stats = build_pyramid("input.png", "out/",
                      PyramidConfig(tile_size=256, min_size=256, resample="bilinear"))
problems = validate_output("out/")  # 空列表表示全部有效
```

## 测试

```bash
python -m pytest tests/ -v -s
```

测试在终端运行并输出校验结果，不打开任何图片窗口。测试图像由代码以
固定种子生成，覆盖：奇数尺寸、边缘 tile 裁剪、nearest/bilinear 两种
缩放及其确定性、增量重建（未变化时零重写）、tile 损坏/缺失/manifest
被篡改后的重新生成、写入中途崩溃的恢复与临时文件清理。

# Architecture

## Isolation boundary

Everything in this desktop implementation lives below `CrossPlatformImageFilter/`.
The iOS application remains the source of truth for its own Swift/Core ML build.
The desktop package shares concepts, prompts, response fields, and model
provenance, but does not import Swift or alter the Xcode target.

## Modules

```text
proxy/
  mitmproxy addon, response eligibility, header rewriting
runtime/
  dependency construction and provider selection
pipeline/
  ordered execution, timing, partial failure, provenance
inference/
  ONNX sessions, YOLO decoding, person detector, NudeNet, MobileCLIP2
image/
  decode/orientation/downscale, crop creation, blur/placeholder encoding
policy/
  editable blocking decision; no policy is embedded in model adapters
cache/
  namespaced memory and SQLite decision cache
diagnostics/
  privacy-preserving JSONL events
 domain/
  typed boxes, detections, classifications, module reports and outcomes
```

## Process model

The default version runs as one process loaded by `mitmdump`. ONNX Runtime owns
native inference threads. A semaphore limits simultaneous image analyses. Model
sessions are created once and reused.

A future packaged GUI may launch the same addon as a child process. Do not add an
HTTP inference server unless there is a demonstrated need. If process isolation
is required later, use a local named pipe or Unix-domain socket with a narrow
binary protocol.

## Request flow

1. Proxy receives a completed HTTP response.
2. Eligibility checks MIME, status, body size, host exclusions, and dimensions.
3. SHA-256 plus policy/model/processing fingerprints identifies reusable decisions.
4. Decode applies EXIF orientation once and downscales once.
5. NudeNet processes the full normalized image regardless of person detection.
6. Person detector produces top-left normalized boxes.
7. Up to 12 largest boxes are expanded and cropped.
8. MobileCLIP2 classifies each crop.
9. NudeNet processes each crop.
10. Crop boxes are mapped to original normalized coordinates.
11. Same-label detections merge at configurable IoU while preserving provenance.
12. Policy selects `allow`, `blur`, `replace`, or `error`; adapters never decide.
13. The proxy returns original bytes or rewritten bytes.

## Coordinate convention

All public desktop response boxes use normalized top-left coordinates:

```text
x=0,y=0 is the upper-left corner
x,y,width,height are in [0,1]
```

Raw model output is converted inside the adapter that owns it. Crop detections
retain both `boundingBoxInCrop` and `boundingBoxInOriginalImage`.

## Partial failure

Modules report independently. NudeNet failure does not erase MobileCLIP2 output,
and person-detector failure does not prevent full-image NudeNet. The policy sees
which evidence is missing. Runtime errors reach the proxy, which applies
`proxy.fail_action`; the default is a valid replacement PNG.

//
//  CameraService.swift
//  ImageRecognitionMealScanning
//
//  Created by Goutam Roy on 13/04/26.
//

import AVFoundation
import CoreImage
import UIKit
import Combine // ✅ REQUIRED

final class CameraService: NSObject, ObservableObject {
    
    let session = AVCaptureSession()
    var previewLayer: AVCaptureVideoPreviewLayer?
    
    private let videoOutput = AVCaptureVideoDataOutput()
    private let imageContext = CIContext()
    
    override init() {
        super.init()
        setupCamera()
    }
    
    private func setupCamera() {
        
        session.beginConfiguration()
        session.sessionPreset = .high
        
        guard let device = AVCaptureDevice.default(for: .video),
              let input = try? AVCaptureDeviceInput(device: device) else {
            return
        }
        
        if session.canAddInput(input) {
            session.addInput(input)
        }
        
        if session.canAddOutput(videoOutput) {
            session.addOutput(videoOutput)
            
            videoOutput.setSampleBufferDelegate(self, queue: DispatchQueue(label: "camera.frame.queue"))
        }
        
        session.commitConfiguration()
        
        // ✅ IMPORTANT: Setup preview layer
        previewLayer = AVCaptureVideoPreviewLayer(session: session)
        previewLayer?.videoGravity = .resizeAspectFill
    }
    
    func startSession() {
        
#if targetEnvironment(simulator)
        print("Camera not supported on simulator")
        return
#else
        
        if !session.isRunning {
            DispatchQueue.main.async {
                self.session.startRunning()
            }
        }
#endif
    }
    
    func stopSession() {
        if session.isRunning {
            session.stopRunning()
        }
    }
}

extension CameraService: AVCaptureVideoDataOutputSampleBufferDelegate {
    
    func captureOutput(_ output: AVCaptureOutput,
                       didOutput sampleBuffer: CMSampleBuffer,
                       from connection: AVCaptureConnection) {
        
        guard let pixelBuffer = CMSampleBufferGetImageBuffer(sampleBuffer) else { return }
        
        let ciImage = CIImage(cvPixelBuffer: pixelBuffer)
        guard let cgImage = imageContext.createCGImage(ciImage, from: ciImage.extent),
              let data = UIImage(cgImage: cgImage).jpegData(compressionQuality: 0.85) else { return }
        Task { @MainActor in
            CameraViewModel.shared.processFrame(imageData: data)
        }
    }
}

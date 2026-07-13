//
//  CameraView.swift
//  ImageRecognitionMealScanning
//
//  Created by Goutam Roy on 13/04/26.
//

import SwiftUI
import AVFoundation

struct CameraView: View {
    
    @StateObject private var cameraService = CameraService()
    @State private var viewModel = CameraViewModel.shared
    
    var body: some View {
        
        ZStack {
            
            // ✅ HANDLE SIMULATOR SAFELY
#if targetEnvironment(simulator)
            
            Color.black
                .ignoresSafeArea()
            
            Text("Camera not available in Simulator")
                .foregroundColor(.white)
                .font(.headline)
            
#else
            
            // ✅ REAL DEVICE CAMERA PREVIEW
            if let previewLayer = cameraService.previewLayer {
                CameraPreview(previewLayer: previewLayer)
                    .ignoresSafeArea()
            } else {
                Color.black.ignoresSafeArea()
            }
            
            // ✅ PREDICTION OVERLAY
            VStack {
                Spacer()
                
                Text(viewModel.prediction)
                    .font(.title2)
                    .padding()
                    .background(Color.black.opacity(0.7))
                    .foregroundColor(.white)
                    .cornerRadius(10)
                    .padding()
            }
            
#endif
        }
        .onAppear {
            cameraService.startSession()
        }
        .onDisappear {
            cameraService.stopSession()
        }
    }
}


// MARK: - Camera Preview (UIKit Bridge)

struct CameraPreview: UIViewRepresentable {
    
    let previewLayer: AVCaptureVideoPreviewLayer
    
    func makeUIView(context: Context) -> UIView {
        let view = UIView()
        view.backgroundColor = .black
        return view
    }
    
    func updateUIView(_ uiView: UIView, context: Context) {
        
        previewLayer.frame = uiView.bounds
        
        // ✅ Prevent duplicate layers
        if !(uiView.layer.sublayers?.contains(previewLayer) ?? false) {
            uiView.layer.sublayers?.forEach { $0.removeFromSuperlayer() }
            uiView.layer.addSublayer(previewLayer)
        }
    }
}

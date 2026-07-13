//
//  ImageRecognitionView.swift
//  ImageRecognitionMealScanning
//
//  Created by Goutam Roy on 13/04/26.
//

//import SwiftUI
//import PhotosUI

import SwiftUI

struct ImageRecognitionView: View {
    
    @State private var viewModel = ImageRecognitionViewModel()
    @State private var selectedImage: UIImage?
    @State private var isImagePickerPresented = false
    
    var body: some View {
        ScrollView {
            VStack(spacing: 20) {
                
                // MARK: Title
                Text("Nudity Detection")
                    .font(.largeTitle)
                    .bold()
                
                // MARK: Image / Empty State
                if let image = selectedImage {
                    Image(uiImage: image)
                        .resizable()
                        .scaledToFit()
                        .frame(height: 250)
                        .cornerRadius(16)
                        .shadow(radius: 5)
                } else {
                    VStack(spacing: 8) {
                        Image(systemName: "photo")
                            .font(.largeTitle)
                            .foregroundColor(.gray)
                        
                        Text("No image selected")
                            .foregroundColor(.gray)
                    }
                    .frame(height: 250)
                }
                
                // MARK: Buttons
                VStack(spacing: 12) {
                    
                    Button("Select Image") {
                        isImagePickerPresented = true
                    }
                    .buttonStyle(.borderedProminent)
                    .frame(maxWidth: .infinity)
                    
                    Button("Analyze Image") {
                        if let image = selectedImage {
                            viewModel.classify(image: image)
                        }
                    }
                    .buttonStyle(.borderedProminent)
                    .tint(.green)
                    .frame(maxWidth: .infinity)
                    .disabled(selectedImage == nil || viewModel.isLoading)
                }
                
                // MARK: Loading Indicator
                if viewModel.isLoading {
                    ProgressView("Analyzing...")
                        .padding()
                }
                
                // MARK: Error Message
                if let error = viewModel.errorMessage {
                    Text(error)
                        .foregroundColor(.red)
                        .padding(.top, 5)
                }
                
                // MARK: Results
                if let decision = viewModel.decision {
                    Text(decision.allowed ? "Allowed" : "Blocked")
                        .font(.headline)
                        .foregroundStyle(decision.allowed ? .green : .red)

                    if viewModel.detections.isEmpty {
                        Text("No NudeNet detections")
                            .foregroundStyle(.secondary)
                    }
                }

                if !viewModel.detections.isEmpty {
                    
                    Text("Detections")
                        .font(.headline)
                        .padding(.top)
                    
                    VStack(spacing: 12) {
                        ForEach(Array(viewModel.detections.enumerated()), id: \.offset) { _, result in
                            
                            VStack(alignment: .leading, spacing: 6) {
                                
                                Text(result.label.replacingOccurrences(of: "_", with: " ").capitalized)
                                    .font(.headline)
                                
                                HStack {
                                    ProgressView(value: result.confidence)
                                        .tint(result.confidence > 0.5 ? .green : .blue)
                                    
                                    Text("\(Int(result.confidence * 100))%")
                                        .foregroundColor(.gray)
                                }
                            }
                            .padding()
                            .background(Color(.systemGray6))
                            .cornerRadius(12)
                            .shadow(radius: 2)
                        }
                    }
                    .animation(.easeInOut, value: viewModel.detections)
                }
                
                Spacer()
            }
            .padding()
        }
        .navigationBarTitleDisplayMode(.inline)
        .sheet(isPresented: $isImagePickerPresented) {
            ImagePicker(image: $selectedImage)
        }
    }
}

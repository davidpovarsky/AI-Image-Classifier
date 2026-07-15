//
//  ContentView.swift
//  AI-Image-Classifier
//
//  Created by Goutam Roy on 14/04/26.
//

import SwiftUI

struct ContentView: View {
    
    var body: some View {
        NavigationStack {
            
            VStack(spacing: 20) {
                
                // MARK: App Title
                Text("AI Image Classifier")
                    .font(.largeTitle)
                    .bold()
                
                // MARK: Navigation Buttons
                
                NavigationLink("Analyze Image") {
                    ImageRecognitionView()
                }
                .buttonStyle(.borderedProminent)
                
                NavigationLink("Voice Assistant") {
                    VoiceView()
                }
                .buttonStyle(.bordered)
                
                NavigationLink("Live Camera Scanner") {
                    CameraView()
                }
                .buttonStyle(.borderedProminent)
                .tint(.green)

                NavigationLink("Local Server") {
                    LocalServerView()
                }
                .buttonStyle(.bordered)

                NavigationLink("Diagnostics") {
                    DiagnosticsView()
                }
                .buttonStyle(.bordered)
                
                Divider()
                
                // MARK: Empty State
                
                Text("Recent Results")
                    .font(.headline)
                
                Text("Start analyzing images to see results")
                    .foregroundColor(.gray)
                
                Spacer()
            }
            .padding()
        }
    }
}

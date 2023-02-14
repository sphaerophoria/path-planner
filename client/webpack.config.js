const HtmlWebpackPlugin = require('html-webpack-plugin');
const CopyPlugin = require('copy-webpack-plugin');
const WasmPackPlugin = require('@wasm-tool/wasm-pack-plugin');
const path = require('path');

module.exports = {
  mode: 'development',
  entry: './src/index.js',
  output: {
    clean: true,
    path: path.resolve(__dirname, 'www'),
    filename: 'path-planner.bundle.js',
  },
  experiments: {
    asyncWebAssembly: true

  },
  plugins: [
    new HtmlWebpackPlugin({
      filename: 'index.html',
      template: './src/index.html'
    }),
    new CopyPlugin({
      patterns: [
        { from: "./src/index.css", to: "index.css" },
        { from: "./src/data.json", to: "data.json" }
      ]
    }),
    new WasmPackPlugin({
      crateDirectory: "./src/rust"
    })
  ]
};

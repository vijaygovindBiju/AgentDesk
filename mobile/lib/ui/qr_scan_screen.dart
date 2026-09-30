import 'dart:async';
import 'package:flutter/material.dart';
import 'package:mobile_scanner/mobile_scanner.dart';

import '../services/connection_service.dart';
import '../services/pairing_client.dart';

class QRScanScreen extends StatefulWidget {
  final ConnectionService connection;
  final PairingClient pairingClient;
  final bool autoConnect;

  const QRScanScreen({
    super.key,
    required this.connection,
    this.pairingClient = const PairingClient(),
    this.autoConnect = true,
  });

  @override
  State<QRScanScreen> createState() => _QRScanScreenState();
}

class _QRScanScreenState extends State<QRScanScreen> {
  late final MobileScannerController _controller;
  bool _isPairing = false;
  String? _pairingStatusMessage;
  String? _errorMessage;
  bool _torchEnabled = false;

  @override
  void initState() {
    super.initState();
    _controller = MobileScannerController(
      detectionSpeed: DetectionSpeed.noDuplicates,
      facing: CameraFacing.back,
      torchEnabled: false,
    );
  }

  @override
  void dispose() {
    _controller.dispose();
    super.dispose();
  }

  Future<void> _handleBarcode(BarcodeCapture capture) async {
    if (_isPairing) return;

    for (final barcode in capture.barcodes) {
      final rawValue = barcode.rawValue;
      if (rawValue != null && rawValue.isNotEmpty) {
        await _processScannedData(rawValue);
        break;
      }
    }
  }

  Future<void> _processScannedData(String rawValue) async {
    final payload = PairingPayload.tryParse(rawValue);
    if (payload == null) {
      if (mounted) {
        ScaffoldMessenger.of(context).showSnackBar(
          const SnackBar(
            content: Text('Scanned QR code is not a valid AgentDesk pairing code.'),
            duration: Duration(seconds: 2),
          ),
        );
      }
      return;
    }

    setState(() {
      _isPairing = true;
      _pairingStatusMessage = 'Pairing with laptop...';
      _errorMessage = null;
    });

    try {
      final deviceId = widget.connection.deviceId.isNotEmpty
          ? widget.connection.deviceId
          : 'android-client-${DateTime.now().millisecondsSinceEpoch % 10000}';
      const deviceName = 'AgentDesk Android';

      final result = await widget.pairingClient.pair(
        payload: payload,
        deviceId: deviceId,
        deviceName: deviceName,
      );

      if (!mounted) return;

      if (result.success) {
        setState(() {
          _pairingStatusMessage = '✓ Paired successfully!';
        });

        await widget.connection.savePairing(
          instanceId: result.instanceId!,
          deviceToken: result.deviceToken!,
          deviceId: deviceId,
          fingerprint: result.fingerprint!,
          relayUrl: result.relayUrl ?? payload.relayUrl,
          autoConnect: widget.autoConnect,
        );

        if (mounted) {
          ScaffoldMessenger.of(context).showSnackBar(
            SnackBar(
              content: Text('Connected to laptop (${result.instanceId})'),
              backgroundColor: Colors.green.shade700,
            ),
          );
          Navigator.of(context).pop(true);
        }
      } else {
        setState(() {
          _isPairing = false;
          _errorMessage = result.error ?? 'Pairing failed';
        });
      }
    } catch (e) {
      if (!mounted) return;
      setState(() {
        _isPairing = false;
        _errorMessage = 'Pairing error: $e';
      });
    }
  }

  void _showManualSimulationDialog() {
    final textController = TextEditingController();
    showDialog(
      context: context,
      builder: (ctx) => AlertDialog(
        title: const Text('Simulate QR Code Scan'),
        content: TextField(
          key: const Key('simulated_qr_input'),
          controller: textController,
          decoration: const InputDecoration(
            labelText: 'Paste agentdesk://pair?... URI',
            hintText: 'agentdesk://pair?v=1&...',
            border: OutlineInputBorder(),
          ),
          maxLines: 3,
        ),
        actions: [
          TextButton(
            onPressed: () => Navigator.of(ctx).pop(),
            child: const Text('Cancel'),
          ),
          FilledButton(
            key: const Key('simulated_qr_submit'),
            onPressed: () {
              final text = textController.text.trim();
              Navigator.of(ctx).pop();
              if (text.isNotEmpty) {
                _processScannedData(text);
              }
            },
            child: const Text('Pair'),
          ),
        ],
      ),
    );
  }

  @override
  Widget build(BuildContext context) {
    return Scaffold(
      appBar: AppBar(
        title: const Text('Pair with Laptop'),
        actions: [
          IconButton(
            icon: Icon(_torchEnabled ? Icons.flash_on : Icons.flash_off),
            tooltip: 'Toggle Flashlight',
            onPressed: () async {
              await _controller.toggleTorch();
              setState(() => _torchEnabled = !_torchEnabled);
            },
          ),
          IconButton(
            key: const Key('simulate_qr_button'),
            icon: const Icon(Icons.paste),
            tooltip: 'Paste Pairing URI',
            onPressed: _showManualSimulationDialog,
          ),
        ],
      ),
      body: Stack(
        children: [
          MobileScanner(
            controller: _controller,
            onDetect: _handleBarcode,
            errorBuilder: (context, error) {
              return Center(
                child: Padding(
                  padding: const EdgeInsets.all(24),
                  child: Column(
                    mainAxisAlignment: MainAxisAlignment.center,
                    children: [
                      Icon(
                        Icons.camera_alt_outlined,
                        size: 64,
                        color: Theme.of(context).colorScheme.error,
                      ),
                      const SizedBox(height: 16),
                      Text(
                        'Camera Unavailable',
                        style: Theme.of(context).textTheme.titleLarge,
                      ),
                      const SizedBox(height: 8),
                      Text(
                        'Camera permission is required to scan the pairing QR code. '
                        'Please allow camera access or use manual configuration.\n\n($error)',
                        textAlign: TextAlign.center,
                        style: Theme.of(context).textTheme.bodyMedium,
                      ),
                      const SizedBox(height: 24),
                      FilledButton.icon(
                        icon: const Icon(Icons.refresh),
                        label: const Text('Try Again'),
                        onPressed: () => _controller.start(),
                      ),
                      const SizedBox(height: 12),
                      OutlinedButton(
                        onPressed: _showManualSimulationDialog,
                        child: const Text('Enter Code Manually'),
                      ),
                    ],
                  ),
                ),
              );
            },
          ),

          // Scanner Target Frame Overlay
          IgnorePointer(
            child: Center(
              child: Container(
                width: 260,
                height: 260,
                decoration: BoxDecoration(
                  border: Border.all(
                    color: Theme.of(context).colorScheme.primary,
                    width: 3,
                  ),
                  borderRadius: BorderRadius.circular(16),
                ),
              ),
            ),
          ),

          // Helper guidance banner at bottom
          Align(
            alignment: Alignment.bottomCenter,
            child: Container(
              width: double.infinity,
              padding: const EdgeInsets.symmetric(horizontal: 24, vertical: 20),
              color: Colors.black87,
              child: Column(
                mainAxisSize: MainAxisSize.min,
                children: [
                  const Text(
                    'Point camera at the QR code in your laptop terminal',
                    textAlign: TextAlign.center,
                    style: TextStyle(
                      color: Colors.white,
                      fontSize: 15,
                      fontWeight: FontWeight.w600,
                    ),
                  ),
                  const SizedBox(height: 4),
                  const Text(
                    'Run "agentdesk pair" on your laptop to display the code.',
                    textAlign: TextAlign.center,
                    style: TextStyle(color: Colors.white70, fontSize: 13),
                  ),
                  if (_errorMessage != null) ...[
                    const SizedBox(height: 12),
                    Container(
                      padding: const EdgeInsets.all(8),
                      decoration: BoxDecoration(
                        color: Colors.red.shade900.withValues(alpha: 0.8),
                        borderRadius: BorderRadius.circular(8),
                      ),
                      child: Text(
                        _errorMessage!,
                        textAlign: TextAlign.center,
                        style: const TextStyle(color: Colors.white, fontSize: 13),
                      ),
                    ),
                  ],
                ],
              ),
            ),
          ),

          // Pairing in progress overlay
          if (_isPairing)
            Container(
              color: Colors.black.withValues(alpha: 0.8),
              child: Center(
                child: Card(
                  elevation: 8,
                  child: Padding(
                    padding: const EdgeInsets.all(24),
                    child: Column(
                      mainAxisSize: MainAxisSize.min,
                      children: [
                        const CircularProgressIndicator(),
                        const SizedBox(height: 20),
                        Text(
                          _pairingStatusMessage ?? 'Pairing...',
                          style: const TextStyle(
                            fontSize: 16,
                            fontWeight: FontWeight.bold,
                          ),
                        ),
                      ],
                    ),
                  ),
                ),
              ),
            ),
        ],
      ),
    );
  }
}

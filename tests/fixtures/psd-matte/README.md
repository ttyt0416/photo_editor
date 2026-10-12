# RGB 합성 미리보기의 흰 배경 표현

별도 Python struct/zlib 인코더 `../generate_psd_matte_inputs.py`로 만든 48개 평면 입력이다. RGB 8/16/32비트 × PSD/PSB × 네 가지 압축 × 알파/불투명을 포함하며 알파0/1·최소 정수 한 단계·1/65535·HDR/음수/signed zero를 사용한다. 이전의 straight RGB 합성 표현24개도 함께 보존했다. 원본 수치와 SHA-256은 manifest.json에 있다. 합성 파일이며 실제 Photoshop 예제/왕복을 뜻하지 않는다.

RGB 합성 평면은 색×알파+(1−알파)로 저장한다. 원래 레이어 평면은 비연관 RGB를 유지한다. [psd-tools NumPy 읽기 구현](https://github.com/psd-tools/psd-tools/blob/main/src/psd_tools/api/numpy_io.py)의 흰 배경 제거와 대조하고 설치된1.10.9로 표시 값을 확인했다. 해당 버전의32비트 NumPy 보기가 읽기 전용 byte view를 수정하려는 문제는 독립 복원한 동일 바이트를 writable raw buffer에 넣는 검사 어댑터로 피했다. 설치 라이브러리나 샘플은 변경하지 않는다.

낮은 알파의 합성 평면은 정수/float 양자화로 나눗셈 오차가 확대되므로 원래 레이어 비트의 정확성, 저장된 합성 평면, 복원한 색×알파를 분리해 확인한다. 전체 합성만 있는 외부 파일의 알파0 색은 검정으로 정규화하며 원래 숨은 색을 복구했다고 주장하지 않는다. Rust 원본 레이어는 저장·다시 읽기에서 비트를 유지한다. `../verify_psd_matte_outputs.py`는48개 Rust 출력의 전체 채널/미리보기와 평면 입력을 독립 비교한다.

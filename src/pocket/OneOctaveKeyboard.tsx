const WHITE_KEYS = [0, 2, 4, 5, 7, 9, 11] as const;
const BLACK_KEYS = [1, 3, 6, 8, 10] as const;
const BLACK_POSITIONS = [1, 2, 4, 5, 6] as const;
const NOTE_NAMES = ["C", "C♯", "D", "D♯", "E", "F", "F♯", "G", "G♯", "A", "A♯", "B"];

interface Props {
  octave: number;
  showLabels: boolean;
  onNote: (midi: number) => void;
}

export function midiFor(octave: number, semitone: number) {
  return (octave + 1) * 12 + semitone;
}

function Key({ midi, semitone, black, showLabels, onNote }: { midi: number; semitone: number; black: boolean; showLabels: boolean; onNote: (midi: number) => void }) {
  const name = `${NOTE_NAMES[semitone]}${Math.floor(midi / 12) - 1}`;
  return (
    <button
      type="button"
      className={`pocket-key ${black ? "pocket-key-black" : "pocket-key-white"}`}
      aria-label={`Play ${name}`}
      onPointerDown={(event) => {
        event.preventDefault();
        onNote(midi);
      }}
      onClick={(event) => {
        // Keyboard activation has no pointer event; pointer clicks were handled above.
        if (event.detail === 0) onNote(midi);
      }}
    >
      {showLabels ? name : null}
    </button>
  );
}

export function OneOctaveKeyboard({ octave, showLabels, onNote }: Props) {
  return (
    <div className="pocket-keyboard" aria-label={`Piano keyboard C${octave} through B${octave}`}>
      <div className="pocket-white-keys">
        {WHITE_KEYS.map((semitone) => (
          <Key key={semitone} midi={midiFor(octave, semitone)} semitone={semitone} black={false} showLabels={showLabels} onNote={onNote} />
        ))}
      </div>
      {BLACK_KEYS.map((semitone, index) => (
        <div key={semitone} className="pocket-black-slot" style={{ left: `${(BLACK_POSITIONS[index] / 7) * 100}%` }}>
          <Key midi={midiFor(octave, semitone)} semitone={semitone} black showLabels={showLabels} onNote={onNote} />
        </div>
      ))}
    </div>
  );
}

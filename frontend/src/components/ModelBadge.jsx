import { useState, useEffect } from 'react';
import { Cpu } from 'lucide-react';
import { fetchModels } from '../api/client';

export default function ModelBadge() {
  const [modelName, setModelName] = useState(null);

  useEffect(() => {
    fetchModels()
      .then((models) => {
        if (models.length > 0) setModelName(models[0].id);
      })
      .catch(() => {});
  }, []);

  return (
    <div className="flex items-center gap-1.5 rounded-full bg-titan-surface px-3 py-1 text-xs text-titan-text-muted">
      <Cpu size={12} />
      {modelName || 'Loading...'}
    </div>
  );
}
